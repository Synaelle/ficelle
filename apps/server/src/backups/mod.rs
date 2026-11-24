// SPDX-License-Identifier: AGPL-3.0-or-later

//! The instance's backups, set up by an admin: the folder on the server,
//! the password, how many to keep. Runs daily in the background, the admin
//! page shows how they went, a failure is logged. People back up their own
//! notes from their apps.

pub mod instance;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use app_repo::{FolderTarget, KdfParams, Policy, Repo, Target};
use base64::Engine;
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::db::DbError;
use crate::keys::MasterKey;
use crate::routes::AppState;
use instance::RunReport;

/// The folder backups get inside the chosen one.
fn folder_name() -> String {
    format!("{} server backups", app_brand::DISPLAY_NAME)
}

/// What the backups folder holds, for whoever opens it.
fn readme() -> String {
    let (app, binary) = (app_brand::DISPLAY_NAME, app_brand::SERVER_BINARY);
    format!(
        "These are backups of a {app} server, encrypted with the backup password.\n\n\
         The files only work together, so do not rename, edit or remove any of them.\n\n\
         To restore, stop the server and run\n\
         {binary} backup restore --target <target.json> --into <empty folder>\n\
         with the backup password in {}_BACKUP_PASSWORD. The target file is the one shown \
         on the admin page, Backups, under To restore the whole instance.\n",
        app_brand::ENV_PREFIX
    )
}

/// How many backups stay when nothing else is said.
pub const DEFAULT_KEEP: u32 = 30;
/// The most that may be kept.
pub const MAX_KEEP: u32 = 100;

/// What keeps the newest `keep` backups and nothing else.
pub fn keeping(keep: u32) -> Policy {
    Policy {
        last: keep.clamp(1, MAX_KEEP),
        daily: 0,
        weekly: 0,
        monthly: 0,
        yearly: 0,
    }
}

/// Where the instance's backups go.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ServerTarget {
    /// A folder the server can write to: another disk, a mounted share.
    Folder {
        /// Its path on the server.
        path: String,
    },
}

/// Places earlier versions also backed up to. Settings naming one are
/// kept but read as no backups, so the admin page asks for a folder.
const DROPPED_KINDS: [&str; 3] = ["s3", "webdav", "sftp"];

/// The dropped place is logged once, not at every look of the schedule.
static DROPPED_WARNED: AtomicBool = AtomicBool::new(false);

/// A folder the admin page may back up to: an absolute path that is not
/// there yet, empty, or holding a repository already (its `config` file,
/// in the folder itself or in its named backups folder). A folder
/// holding anything else, like the server's own files or a system folder,
/// is never written into or pruned.
fn folder_allowed(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("the folder must be a full path, like /backups".into());
    }
    let Ok(mut entries) = std::fs::read_dir(path) else {
        return if path.exists() {
            Err("this is not a folder the server can read".into())
        } else {
            Ok(())
        };
    };
    if entries.next().is_none()
        || path.join("config").is_file()
        || path.join(folder_name()).join("config").is_file()
    {
        Ok(())
    } else {
        Err("this folder holds other files: choose an empty folder or a backup".into())
    }
}

impl ServerTarget {
    /// The target to use.
    ///
    /// # Errors
    ///
    /// An incomplete config, in words.
    pub fn open(&self) -> Result<Arc<dyn Target>, String> {
        let Self::Folder { path } = self;
        if path.trim().is_empty() {
            return Err("a folder is needed".into());
        }
        folder_allowed(Path::new(path.trim()))?;
        Ok(Arc::new(FolderTarget::named(
            path.trim(),
            &folder_name(),
            &readme(),
        )))
    }
}

/// How often.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Schedule {
    /// Only when asked.
    Off,
    /// Every hour.
    Hourly,
    /// Every day.
    #[default]
    Daily,
    /// Every week.
    Weekly,
}

impl Schedule {
    pub(crate) fn period_ms(self) -> Option<i64> {
        match self {
            Self::Off => None,
            Self::Hourly => Some(3_600_000),
            Self::Daily => Some(86_400_000),
            Self::Weekly => Some(7 * 86_400_000),
        }
    }
}

/// The instance's backup settings. The admin page sets a daily schedule
/// and a policy of the newest few: settings from before, with another
/// schedule or policy, keep them until a number is chosen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupConfig {
    /// Where.
    pub target: ServerTarget,
    /// The repository's password.
    pub password: String,
    /// When.
    pub schedule: Schedule,
    /// What to keep.
    pub policy: Policy,
    /// The time zone for the policy's days, seconds east of UTC.
    pub offset_seconds: i64,
}

impl BackupConfig {
    /// How many backups stay, `None` for older settings with another
    /// schedule or policy.
    pub fn keep(&self) -> Option<u32> {
        (self.schedule == Schedule::Daily && self.policy == keeping(self.policy.last))
            .then_some(self.policy.last)
    }
}

/// How the runs went.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BackupStatus {
    /// The last run started.
    pub last_run_at: Option<i64>,
    /// The last that went through.
    pub last_success_at: Option<i64>,
    /// Why the last failed.
    pub last_error: Option<String>,
    /// What the last good run did.
    pub last_report: Option<RunReport>,
    /// Snapshots after the last run.
    pub snapshots: u64,
    /// Bytes at the target after the last run.
    pub stored_bytes: u64,
}

fn read_setting<T: serde::de::DeserializeOwned>(
    conn: &Connection,
    key: &str,
) -> Result<Option<T>, DbError> {
    let text: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(text.and_then(|t| serde_json::from_str(&t).ok()))
}

fn write_setting<T: Serialize>(
    conn: &Connection,
    key: &str,
    value: Option<&T>,
) -> Result<(), DbError> {
    match value {
        Some(value) => {
            let text = serde_json::to_string(value).unwrap_or_default();
            conn.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                [key, &text],
            )?;
        }
        None => {
            conn.execute("DELETE FROM settings WHERE key = ?1", [key])?;
        }
    }
    Ok(())
}

/// The settings, None when backups are not set up. They hold the
/// target's credentials and the repository password, so they are kept
/// sealed by the master key; settings from
/// before 0.3.0, kept plain, are sealed the first time they are read.
///
/// # Errors
///
/// Database errors, and [`DbError::Data`] for settings that are stored but
/// do not read back (another master key, a shape this version does not
/// know): backups are then not off, they are broken, and say so.
pub fn config(conn: &Connection, master: &MasterKey) -> Result<Option<BackupConfig>, DbError> {
    let Some(stored) = read_setting::<serde_json::Value>(conn, "backups")? else {
        return Ok(None);
    };
    let unreadable = || DbError::Data("the backup settings cannot be read, set them again".into());
    let plain = stored.get("sealed").is_none();
    let value: serde_json::Value = match stored.get("sealed").and_then(|s| s.as_str()) {
        Some(sealed) => {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(sealed)
                .map_err(|_| unreadable())?;
            let text = master
                .open_setting("backups", &bytes)
                .ok_or_else(unreadable)?;
            serde_json::from_slice(&text).map_err(|_| unreadable())?
        }
        None => stored,
    };
    if let Some(kind) = value["target"]["kind"]
        .as_str()
        .filter(|k| DROPPED_KINDS.contains(k))
    {
        if !DROPPED_WARNED.swap(true, Ordering::SeqCst) {
            tracing::warn!(
                "backups went to {kind}, which this version no longer backs up to. \
                 They are off until a folder is set on the admin page"
            );
        }
        return Ok(None);
    }
    let config: BackupConfig = serde_json::from_value(value).map_err(|_| unreadable())?;
    if plain {
        set_config(conn, master, Some(&config))?;
    }
    Ok(Some(config))
}

/// Sets or clears the settings, sealed.
///
/// # Errors
///
/// Database errors.
pub fn set_config(
    conn: &Connection,
    master: &MasterKey,
    config: Option<&BackupConfig>,
) -> Result<(), DbError> {
    let sealed = config.map(|config| {
        let text = zeroize::Zeroizing::new(serde_json::to_vec(config).unwrap_or_default());
        serde_json::json!({
            "sealed": base64::engine::general_purpose::STANDARD
                .encode(master.seal_setting("backups", &text)),
        })
    });
    write_setting(conn, "backups", sealed.as_ref())
}

/// How the runs went.
///
/// # Errors
///
/// Database errors.
pub fn status(conn: &Connection) -> Result<BackupStatus, DbError> {
    Ok(read_setting(conn, "backups_status")?.unwrap_or_default())
}

/// Records how the runs went.
///
/// # Errors
///
/// Database errors.
pub fn set_status(conn: &Connection, status: &BackupStatus) -> Result<(), DbError> {
    write_setting(conn, "backups_status", Some(status))
}

/// Opens or makes the repository a config names.
///
/// # Errors
///
/// The target refused, the wrong password, in words.
pub fn open(config: &BackupConfig, create: bool, now: i64) -> Result<Repo, String> {
    let target = config.target.open()?;
    if create {
        Repo::init(target, &config.password, KdfParams::DEFAULT, now)
    } else {
        Repo::open(target, &config.password)
    }
    .map_err(|e| match e {
        app_repo::Error::WrongPassword => "wrong password for this backup".into(),
        app_repo::Error::NotFound(_) => {
            "there is no backup at this place: it was moved or the place is not reachable".into()
        }
        other => other.to_string(),
    })
}

/// One run at a time on this server.
static RUNNING: AtomicBool = AtomicBool::new(false);

/// Whether a run is going on.
pub fn running() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

/// The master key's file, to back up with the database when it lives in
/// the data folder. A key kept apart (`storage.master_key_file`, a Docker
/// secret) stays apart: with it in the backups, a stolen backup would read
/// everything. Restoring then needs that
/// file.
fn master_key_path(state: &AppState) -> Option<PathBuf> {
    if !state.config.storage.master_key_file.as_os_str().is_empty() {
        return None;
    }
    let path = state.config.storage.data_dir.join(crate::keys::KEY_FILE);
    path.exists().then_some(path)
}

/// A scratch folder in the data folder, new.
pub fn scratch(state: &AppState, what: &str) -> PathBuf {
    state
        .config
        .storage
        .data_dir
        .join(format!("backup-{what}-{}", state.clock.now()))
}

/// Scratch folders a run, a check or a restore left when the server
/// stopped in the middle: they hold copies of the database, removed at
/// start. Nothing runs yet then.
pub fn remove_leftover_scratch(data_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(data_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with("backup-")
            && entry.file_type().is_ok_and(|t| t.is_dir())
        {
            remove_scratch(&entry.path());
        }
    }
}

fn remove_scratch(path: &Path) {
    if path.exists() {
        let _ = std::fs::remove_dir_all(path);
    }
}

/// Backs the instance up now, then thins the snapshots by the policy.
/// Records how it went, logs a failure.
///
/// # Errors
///
/// Why it failed, in words (recorded too).
pub async fn run(state: &AppState, by: &str) -> Result<BackupStatus, String> {
    if RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("a backup is running already".into());
    }
    let result = run_once(state).await;
    RUNNING.store(false, Ordering::SeqCst);
    let now = state.clock.now();
    let failed = result.as_ref().err().cloned();
    let report = result.as_ref().ok().cloned();
    let by = by.to_owned();
    let status = state
        .db
        .call(move |c| {
            let mut status = status(c)?;
            status.last_run_at = Some(now);
            match (report, failed.as_ref()) {
                (Some((report, snapshots, stored)), _) => {
                    status.last_success_at = Some(now);
                    status.last_error = None;
                    status.last_report = Some(report);
                    status.snapshots = snapshots;
                    status.stored_bytes = stored;
                }
                (None, Some(why)) => status.last_error = Some(why.clone()),
                (None, None) => {}
            }
            set_status(c, &status)?;
            let (name, detail) = match &failed {
                Some(why) => ("backup_failed", format!("{by}: {why}")),
                None => ("backup_done", by.clone()),
            };
            crate::accounts::store::audit(
                c,
                now,
                &crate::accounts::store::Event {
                    name,
                    user_id: None,
                    device_id: None,
                    ip: None,
                    detail: Some(&detail),
                },
            )?;
            Ok(status)
        })
        .await
        .map_err(|e| e.to_string())?;
    if let Some(why) = &status.last_error.clone().filter(|_| result.is_err()) {
        return Err(why.clone());
    }
    Ok(status)
}

async fn run_once(state: &AppState) -> Result<(RunReport, u64, u64), String> {
    let master = state.master.clone();
    let config = state
        .db
        .call(move |c| config(c, &master))
        .await
        .map_err(|e| e.to_string())?
        .ok_or("backups are not set up")?;
    let dir = scratch(state, "run");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let copy = dir.join("server.db");
    let copy_text = copy.to_string_lossy().into_owned();
    let copied = state
        .db
        .call(move |c| {
            c.execute("VACUUM INTO ?1", [&copy_text])?;
            Ok(())
        })
        .await
        .map_err(|e| format!("copying the database: {e}"));
    if let Err(e) = copied {
        remove_scratch(&dir);
        return Err(e);
    }
    let files = state.files.clone();
    let key = master_key_path(state);
    let clock = state.clock.clone();
    let result = tokio::task::spawn_blocking(move || {
        let now = || clock.now();
        let repo = open(&config, false, now())?;
        let report = instance::backup(&repo, &copy, key.as_deref(), files.as_ref(), &now)?;
        let snapshots: Vec<(String, i64)> = repo
            .snapshots()
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter(|s| s.host == instance::HOST)
            .map(|s| (s.id, s.time))
            .collect();
        let kept = app_repo::keep(&snapshots, config.policy, config.offset_seconds);
        let gone: std::collections::HashSet<String> = snapshots
            .into_iter()
            .map(|(id, _)| id)
            .filter(|id| !kept.contains(id))
            .collect();
        if !gone.is_empty() {
            repo.forget(&gone).map_err(|e| e.to_string())?;
            repo.prune(instance::HOST, now())
                .map_err(|e| e.to_string())?;
        }
        let count = repo.snapshots().map_err(|e| e.to_string())?.len() as u64;
        let stored = repo
            .target()
            .list("data/")
            .map_err(|e| e.to_string())?
            .iter()
            .map(|l| l.size)
            .sum();
        Ok::<_, String>((report, count, stored))
    })
    .await
    .map_err(|e| e.to_string())?;
    remove_scratch(&dir);
    result
}

/// Runs backups on their schedule: a look every five minutes, a run when a
/// period passed since the last success (a failed one tried again after
/// half an hour).
pub fn spawn_schedule(state: AppState) {
    tokio::spawn(async move {
        let mut every = tokio::time::interval(std::time::Duration::from_secs(300));
        loop {
            every.tick().await;
            let master = state.master.clone();
            let looked = state
                .db
                .call(move |c| Ok((config(c, &master)?, status(c)?)))
                .await;
            let (config, status) = match looked {
                Ok((Some(config), status)) => (config, status),
                Ok((None, _)) => continue,
                Err(why) => {
                    tracing::warn!("backups cannot run: {why}");
                    continue;
                }
            };
            let Some(period) = config.schedule.period_ms() else {
                continue;
            };
            let now = state.clock.now();
            let since_success = status.last_success_at.map_or(i64::MAX, |t| now - t);
            let since_run = status.last_run_at.map_or(i64::MAX, |t| now - t);
            if since_success >= period
                && since_run >= period.min(30 * 60_000)
                && !running()
                && let Err(why) = run(&state, "schedule").await
            {
                tracing::warn!("backup failed: {why}");
            }
        }
    });
}

#[cfg(test)]
mod folder_tests {
    use super::folder_allowed;

    #[test]
    fn a_folder_place_is_new_empty_or_a_backup() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            folder_allowed(&dir.path().join("new")).is_ok(),
            "not there yet"
        );
        assert!(folder_allowed(dir.path()).is_ok(), "empty");
        std::fs::write(dir.path().join("notes.txt"), b"x").unwrap();
        assert!(folder_allowed(dir.path()).is_err(), "holds other files");
        std::fs::write(dir.path().join("config"), b"{}").unwrap();
        assert!(folder_allowed(dir.path()).is_ok(), "a backup");
        assert!(folder_allowed(std::path::Path::new("relative/path")).is_err());
    }
}

#[cfg(test)]
mod sealed_tests {
    use super::*;
    use crate::db::Db;

    /// The target's secrets and the repository
    /// password are not in the database as text, and old plain settings are
    /// sealed when read.
    #[tokio::test]
    async fn backup_settings_are_sealed_in_the_database() {
        let db = Db::in_memory().unwrap();
        let master = std::sync::Arc::new(MasterKey::generate());
        let m = master.clone();
        let (raw, back, resealed) = db
            .call(move |c| {
                let settings = BackupConfig {
                    target: serde_json::from_value(
                        serde_json::json!({ "kind": "folder", "path": "/somewhere" }),
                    )
                    .unwrap(),
                    password: "the repository password".into(),
                    schedule: Schedule::Daily,
                    policy: Policy::default(),
                    offset_seconds: 0,
                };
                // as 0.2 kept them
                write_setting(c, "backups", Some(&settings))?;
                let back = config(c, &m)?;
                let raw: String = c.query_row(
                    "SELECT value FROM settings WHERE key = 'backups'",
                    [],
                    |r| r.get(0),
                )?;
                set_config(c, &m, back.as_ref())?;
                Ok((raw, back, config(c, &m)?))
            })
            .await
            .unwrap();
        assert!(!raw.contains("the repository password"), "{raw}");
        assert!(!raw.contains("somewhere"), "{raw}");
        assert_eq!(back.unwrap().password, "the repository password");
        assert_eq!(resealed.unwrap().password, "the repository password");
        let other = MasterKey::generate();
        let opened = db.call(move |c| config(c, &other)).await;
        assert!(
            matches!(opened, Err(DbError::Data(_))),
            "another key reads nothing, and says so: {opened:?}"
        );
    }

    /// Settings for a place this version dropped read as no backups, and
    /// stay in the database until the admin sets a folder.
    #[tokio::test]
    async fn a_dropped_place_reads_as_backups_off() {
        let db = Db::in_memory().unwrap();
        let master = std::sync::Arc::new(MasterKey::generate());
        let m = master.clone();
        let (sealed, plain, kept) = db
            .call(move |c| {
                let old = serde_json::json!({
                    "target": { "kind": "sftp", "host": "nas.home", "port": 22,
                        "username": "backup", "path": "/srv", "password": "pw" },
                    "password": "the repository password",
                    "schedule": "daily",
                    "policy": Policy::default(),
                    "offsetSeconds": 0,
                });
                let text = serde_json::to_vec(&old).unwrap();
                let stored = serde_json::json!({
                    "sealed": base64::engine::general_purpose::STANDARD
                        .encode(m.seal_setting("backups", &text)),
                });
                write_setting(c, "backups", Some(&stored))?;
                let sealed = config(c, &m)?;
                // as 0.2 kept them, plain
                write_setting(c, "backups", Some(&old))?;
                let plain = config(c, &m)?;
                Ok((
                    sealed,
                    plain,
                    read_setting::<serde_json::Value>(c, "backups")?,
                ))
            })
            .await
            .unwrap();
        assert!(sealed.is_none());
        assert!(plain.is_none());
        assert_eq!(kept.unwrap()["target"]["kind"], "sftp", "left as it was");
    }
}
