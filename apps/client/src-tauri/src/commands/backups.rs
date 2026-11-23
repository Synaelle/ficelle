// SPDX-License-Identifier: AGPL-3.0-or-later

//! Automatic backups (see `app_core::auto_backup`): the vault's backup file
//! made in a folder on a schedule, the newest few kept, and restored from
//! that folder. The page keeps the schedule (runs happen while the app is
//! open), this side does the work, holding the vault only to read what to
//! back up and to put back what a restore brings.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use app_core::auto_backup::{AutoBackups, DEFAULT_KEEP, MAX_KEEP};
use app_core::backup::Backup;
use app_core::clock::{Clock, SystemClock};
use app_core::crypto::KdfParams;
use app_core::repo::{BackupStatus, Schedule};
use app_core::store::RestoreReport;
use app_core::vault::Vault;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

/// One run at a time on this device.
static RUNNING: AtomicBool = AtomicBool::new(false);

/// The settings as the page sees them: no password.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigView {
    folder: String,
    schedule: Schedule,
    keep: u32,
}

/// Backups on this device.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupsView {
    config: Option<ConfigView>,
    status: BackupStatus,
    running: bool,
}

/// A backup file in the folder.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    name: String,
    size: u64,
    /// Made by the automatic backups (else put there by hand).
    automatic: bool,
}

fn now() -> i64 {
    SystemClock.now()
}

fn view(state: &AppState) -> CommandResult<BackupsView> {
    let (config, status) =
        state.quietly(|s| Ok((s.store.auto_backups()?, s.store.backup_status()?)))?;
    Ok(BackupsView {
        config: config.map(|c| ConfigView {
            folder: c.folder,
            schedule: c.schedule,
            keep: c.keep,
        }),
        status,
        running: RUNNING.load(Ordering::SeqCst),
    })
}

async fn blocking<T: Send + 'static>(
    app: AppHandle,
    f: impl FnOnce(&AppState) -> CommandResult<T> + Send + 'static,
) -> CommandResult<T> {
    tauri::async_runtime::spawn_blocking(move || f(&app.state::<AppState>()))
        .await
        .map_err(|e| CommandError::new("unknown", e.to_string()))?
}

fn config(state: &AppState) -> CommandResult<AutoBackups> {
    state
        .quietly(|s| s.store.auto_backups())?
        .ok_or_else(|| CommandError::new("notFound", "backups are not set up"))
}

/// The folder the files are in: the chosen one, or a folder of its own
/// inside it, as the snapshot backups had (`app_core::repo::folder_name`).
enum Place {
    Dir(PathBuf),
    #[cfg(target_os = "android")]
    Document(crate::documents::DocumentTarget),
}

/// A backup file's name, never a path.
fn backup_name(name: &str) -> CommandResult<&str> {
    let plain = !name.is_empty()
        && !name.starts_with('.')
        && !name.contains(['/', '\\'])
        && name.ends_with(&format!(".{}", app_brand::BACKUP_EXTENSION));
    if plain {
        Ok(name)
    } else {
        Err(CommandError::new("invalid", "not a backup file"))
    }
}

impl Place {
    fn of(folder: &str) -> CommandResult<Self> {
        #[cfg(target_os = "android")]
        if crate::picked::is_document(folder) {
            return Ok(Self::Document(crate::documents::DocumentTarget::named(
                folder,
            )?));
        }
        if folder.trim().is_empty() {
            return Err(CommandError::new("invalid", "choose a folder"));
        }
        let chosen = PathBuf::from(folder);
        let name = app_core::repo::folder_name(folder);
        let own = chosen.join("config").is_file()
            || chosen.file_name().is_some_and(|n| n == name.as_str());
        Ok(Self::Dir(if own { chosen } else { chosen.join(name) }))
    }

    /// The backup files there, sizes with them.
    fn list(&self) -> CommandResult<Vec<(String, u64)>> {
        let ours = |name: &str| backup_name(name).is_ok();
        match self {
            Self::Dir(dir) => {
                let entries = match std::fs::read_dir(dir) {
                    Ok(entries) => entries,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
                    Err(e) => return Err(readable(e.into())),
                };
                let mut out = Vec::new();
                for entry in entries {
                    let entry = entry.map_err(|e| readable(e.into()))?;
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if ours(&name) {
                        let size = entry.metadata().map_or(0, |m| m.len());
                        out.push((name, size));
                    }
                }
                Ok(out)
            }
            #[cfg(target_os = "android")]
            Self::Document(target) => Ok(target
                .list()
                .map_err(readable)?
                .into_iter()
                .filter(|(path, _)| !path.contains('/') && ours(path))
                .collect()),
        }
    }

    /// Writes the file `name`, beside it first, then renamed: a run that
    /// fails leaves no half backup.
    fn write<T>(
        &self,
        name: &str,
        fill: impl FnOnce(&mut std::fs::File) -> app_core::Result<T>,
    ) -> CommandResult<T> {
        match self {
            Self::Dir(dir) => {
                std::fs::create_dir_all(dir).map_err(app_core::Error::from)?;
                let path = dir.join(name);
                let partial = dir.join(format!("{name}.partial"));
                let written = std::fs::File::create(&partial)
                    .map_err(app_core::Error::from)
                    .and_then(|mut file| {
                        let filled = fill(&mut file)?;
                        file.sync_all()?;
                        Ok(filled)
                    })
                    .and_then(|filled| {
                        std::fs::rename(&partial, &path)?;
                        Ok(filled)
                    });
                if written.is_err() {
                    let _ = std::fs::remove_file(&partial);
                }
                Ok(written?)
            }
            #[cfg(target_os = "android")]
            Self::Document(target) => Ok(target.put_with(name, fill)?),
        }
    }

    fn open(&self, name: &str) -> CommandResult<std::fs::File> {
        let name = backup_name(name)?;
        Ok(match self {
            Self::Dir(dir) => std::fs::File::open(dir.join(name)).map_err(app_core::Error::from)?,
            #[cfg(target_os = "android")]
            Self::Document(target) => target.open_named(name)?,
        })
    }

    fn delete(&self, name: &str) -> CommandResult<()> {
        let name = backup_name(name)?;
        match self {
            Self::Dir(dir) => match std::fs::remove_file(dir.join(name)) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    Err(app_core::Error::from(e).into())
                }
                _ => Ok(()),
            },
            #[cfg(target_os = "android")]
            Self::Document(target) => Ok(target.delete(name)?),
        }
    }
}

/// This device's backups.
#[tauri::command]
pub async fn backups_get(state: State<'_, AppState>) -> CommandResult<BackupsView> {
    view(&state)
}

/// Turns automatic backups on, in `folder`. Notes with a password give it
/// here, checked: the backups are sealed with it. A run follows from the
/// page.
#[tauri::command]
pub async fn backups_turn_on(
    app: AppHandle,
    folder: String,
    password: Option<String>,
) -> CommandResult<BackupsView> {
    blocking(app, move |state| {
        let encrypted = state
            .session()
            .as_ref()
            .ok_or_else(CommandError::locked)?
            .vault
            .is_encrypted();
        let password = if encrypted {
            let given = password.filter(|p| !p.is_empty()).ok_or_else(unprotected)?;
            if Vault::open(&state.dir)?.key_with_password(&given).is_err() {
                return Err(CommandError::new("wrongPassword", "not the notes password"));
            }
            Some(given)
        } else {
            None
        };
        // the folder must take a file now, not at the first run
        Place::of(&folder)?.list()?;
        let old = state.quietly(|s| s.store.auto_backups())?;
        let config = AutoBackups {
            folder,
            schedule: old.as_ref().map_or(Schedule::Daily, |o| o.schedule),
            keep: old.as_ref().map_or(DEFAULT_KEEP, |o| o.keep),
            password,
        };
        state.quietly(|s| {
            s.store.set_auto_backups(Some(&config))?;
            // a new place: its history starts now
            s.store.set_backup_status(&BackupStatus::default())
        })?;
        view(state)
    })
    .await
}

/// When to back up and how many to keep.
#[tauri::command]
pub async fn backups_settings(
    state: State<'_, AppState>,
    schedule: Schedule,
    keep: u32,
) -> CommandResult<BackupsView> {
    let mut config = config(&state)?;
    config.schedule = schedule;
    config.keep = keep.clamp(1, MAX_KEEP);
    state.quietly(|s| s.store.set_auto_backups(Some(&config)))?;
    view(&state)
}

/// Stops backing up. The files stay where they are.
#[tauri::command]
pub async fn backups_turn_off(state: State<'_, AppState>) -> CommandResult<BackupsView> {
    state.quietly(|s| s.store.set_auto_backups(None))?;
    view(&state)
}

/// Android keeps a new install out of the files an earlier one made: they
/// are there, never "no backups". The page then asks for the folder in the
/// system's picker (`backups_pick_folder`).
fn readable(err: app_core::Error) -> CommandError {
    match err {
        app_core::Error::Io(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            CommandError::new("unreadable", "the app may not read this folder")
        }
        e => e.into(),
    }
}

/// Asks for the phone's backups folder in the system's picker, which lets
/// the app reach what an earlier install made there. Its address, none when
/// the person backed out.
#[tauri::command]
pub async fn backups_pick_folder(app: AppHandle) -> CommandResult<Option<String>> {
    blocking(app, |_| {
        #[cfg(target_os = "android")]
        return Ok(crate::documents::pick()?);
        #[cfg(not(target_os = "android"))]
        Err(CommandError::new(
            "unsupported",
            "only a phone picks its backups folder",
        ))
    })
    .await
}

fn unprotected() -> CommandError {
    CommandError::new(
        "unprotected",
        "the notes have a password, the backups need it",
    )
}

/// The notes got a password, or another one: the next backups are sealed
/// with it. Those made before keep the password they were made with, so the
/// next run makes a file even when no note changed.
///
/// # Errors
///
/// When the settings cannot be written.
pub async fn protect(app: AppHandle, password: String) -> CommandResult<()> {
    blocking(app, move |state| {
        let Some(mut config) = state.quietly(|s| s.store.auto_backups())? else {
            return Ok(());
        };
        config.password = Some(password);
        state.quietly(|s| {
            s.store.set_auto_backups(Some(&config))?;
            let mut status = s.store.backup_status()?;
            status.last_digest = None;
            s.store.set_backup_status(&status)
        })
    })
    .await
}

fn run(state: &AppState, offset_seconds: i64) -> CommandResult<()> {
    let config = config(state)?;
    let encrypted = state
        .session()
        .as_ref()
        .is_some_and(|s| s.vault.is_encrypted());
    if encrypted && config.password.is_none() {
        return Err(unprotected());
    }
    // read with the vault held, written without it: the notes stay usable
    let (data, blobs, last) = state.quietly(|s| {
        Ok((
            s.store.backup_data()?,
            s.blobs.clone(),
            s.store.backup_status()?.last_digest,
        ))
    })?;
    let mut present = Vec::new();
    for sha in data.file_hashes() {
        if blobs.has(&sha)? {
            present.push(sha);
        }
    }
    present.sort();
    let digest = app_core::auto_backup::digest(&data, &present)?;
    let place = Place::of(&config.folder)?;
    let mut files = place.list()?;
    let newest = files
        .iter()
        .any(|(name, _)| app_core::auto_backup::is_automatic(name));
    let mut damaged = None;
    // nothing changed since the last one, which is still there
    if last.as_deref() != Some(&digest) || !newest {
        let name = app_core::auto_backup::file_name(now(), offset_seconds);
        let manifest = place.write(&name, |file| {
            app_core::backup::write_backup_data(
                &data,
                &blobs,
                std::io::BufWriter::new(file),
                config.password.as_deref(),
                KdfParams::DEFAULT,
            )
        })?;
        damaged = Some(manifest.damaged);
        let names: Vec<String> = place.list()?.into_iter().map(|(n, _)| n).collect();
        for gone in app_core::auto_backup::beyond(&names, config.keep) {
            place.delete(&gone)?;
        }
        files = place.list()?;
    }
    let automatic: Vec<&(String, u64)> = files
        .iter()
        .filter(|(name, _)| app_core::auto_backup::is_automatic(name))
        .collect();
    state.quietly(|s| {
        let mut status = s.store.backup_status()?;
        status.last_success_at = Some(now());
        status.last_error = None;
        status.last_error_kind = None;
        status.last_digest = Some(digest);
        if let Some(damaged) = damaged {
            status.damaged = damaged;
        }
        status.files = automatic.len() as u64;
        status.stored_bytes = automatic.iter().map(|(_, size)| size).sum();
        s.store.set_backup_status(&status)
    })?;
    Ok(())
}

/// Backs up now: a new file when something changed since the last one,
/// then the automatic files beyond those kept go. `offset_seconds`: the
/// time zone, for the file's name.
#[tauri::command]
pub async fn backups_run(app: AppHandle, offset_seconds: i64) -> CommandResult<BackupsView> {
    if RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(CommandError::new("invalid", "a backup is running already"));
    }
    blocking(app, move |state| {
        let started = state.quietly(|s| {
            let mut status = s.store.backup_status()?;
            status.last_run_at = Some(now());
            s.store.set_backup_status(&status)
        });
        let result = started.and_then(|()| run(state, offset_seconds));
        RUNNING.store(false, Ordering::SeqCst);
        if let Err(e) = &result {
            // the vault may have been locked meanwhile: the error is told
            // to the page either way
            let _ = state.quietly(|s| {
                let mut status = s.store.backup_status()?;
                status.last_error = Some(e.message().to_owned());
                status.last_error_kind = Some(e.kind().to_owned());
                s.store.set_backup_status(&status)
            });
        }
        result?;
        view(state)
    })
    .await
}

/// The backup files in the folder, newest first.
#[tauri::command]
pub async fn backups_list(app: AppHandle) -> CommandResult<Vec<FileView>> {
    blocking(app, |state| {
        let mut files: Vec<FileView> = Place::of(&config(state)?.folder)?
            .list()?
            .into_iter()
            .map(|(name, size)| FileView {
                automatic: app_core::auto_backup::is_automatic(&name),
                name,
                size,
            })
            .collect();
        // the automatic ones by their time, the others after them
        files.sort_by(|a, b| b.automatic.cmp(&a.automatic).then(b.name.cmp(&a.name)));
        Ok(files)
    })
    .await
}

/// Puts a backup file of the folder back: what is missing here comes back
/// with its ids, what is here stays as it is. `password` for a file made
/// with another than the current one.
#[tauri::command]
pub async fn backups_restore(
    app: AppHandle,
    name: String,
    password: Option<String>,
) -> CommandResult<RestoreReport> {
    blocking(app, move |state| {
        let config = config(state)?;
        let file = Place::of(&config.folder)?.open(&name)?;
        let password = password.filter(|p| !p.is_empty()).or(config.password);
        let mut backup = Backup::open_file(file, password.as_deref())?;
        state.with(|s| backup.restore(&mut s.store, &s.blobs))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_takes_files_lists_them_and_lets_them_go() {
        let chosen = std::env::temp_dir().join(format!("auto-backups-{}", std::process::id()));
        std::fs::create_dir_all(&chosen).unwrap();
        let place = Place::of(&chosen.to_string_lossy()).unwrap();
        let name = app_core::auto_backup::file_name(1_792_742_400_000, 0);
        place
            .write(&name, |file| {
                use std::io::Write;
                file.write_all(b"backup")?;
                Ok(())
            })
            .unwrap();
        // a failed write leaves nothing, not even its partial file
        let failed = place.write("other.ficelle", |_| -> app_core::Result<()> {
            Err(app_core::Error::Invalid("stopped".into()))
        });
        assert!(failed.is_err());
        match &place {
            Place::Dir(dir) => {
                assert!(dir.ends_with(app_core::repo::folder_name(&chosen.to_string_lossy())));
            }
        }
        assert_eq!(place.list().unwrap(), vec![(name.clone(), 6)]);
        assert!(place.open("../secrets.ficelle").is_err());
        place.delete(&name).unwrap();
        assert!(place.list().unwrap().is_empty());
        std::fs::remove_dir_all(chosen).unwrap();
    }
}
