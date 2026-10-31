// SPDX-License-Identifier: AGPL-3.0-or-later

//! Scheduled backups of the vault into a repository (see `app_core::repo`):
//! setting them up, running them, their history, restoring. The page keeps
//! the schedule (runs happen while the app is open), this side does the
//! work, holding the vault only to read what to back up and to write what
//! comes back: the upload and download run without it.

use std::sync::atomic::{AtomicBool, Ordering};

use app_core::clock::{Clock, SystemClock};
use app_core::repo::{
    BackupConfig, BackupStatus, CheckReport, KdfParams, Policy, Repo, Schedule, TargetConfig,
    TestRestore,
};
use app_core::store::RestoreReport;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

/// One run at a time on this device.
static RUNNING: AtomicBool = AtomicBool::new(false);

/// The settings as the page sees them: no secrets.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigView {
    target: TargetConfig,
    schedule: Schedule,
    policy: Policy,
    host: String,
}

/// Backups on this device.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupsView {
    config: Option<ConfigView>,
    status: BackupStatus,
    running: bool,
    /// Where backups may go on this platform.
    kinds: Vec<&'static str>,
}

/// A snapshot as listed.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotView {
    id: String,
    time: i64,
    host: String,
    app_version: String,
    counts: std::collections::BTreeMap<String, u64>,
    size: u64,
}

fn now() -> i64 {
    SystemClock.now()
}

fn kinds() -> Vec<&'static str> {
    // the phones' pickers give no path: a phone's folder is the one it
    // names itself (`SystemInfo.backupFolder` in MainActivity.kt)
    vec!["folder", "s3", "webdav"]
}

fn view(state: &AppState) -> CommandResult<BackupsView> {
    let (config, status) =
        state.quietly(|s| Ok((s.store.backup_config()?, s.store.backup_status()?)))?;
    Ok(BackupsView {
        config: config.map(|c| ConfigView {
            target: c.target.without_secrets(),
            schedule: c.schedule,
            policy: c.policy,
            host: c.host,
        }),
        status,
        running: RUNNING.load(Ordering::SeqCst),
        kinds: kinds(),
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

fn config(state: &AppState) -> CommandResult<BackupConfig> {
    state
        .quietly(|s| s.store.backup_config())?
        .ok_or_else(|| CommandError::new("notFound", "backups are not set up"))
}

fn open(config: &BackupConfig) -> CommandResult<Repo> {
    Ok(Repo::open(config.target.open()?, &config.password).map_err(app_core::Error::from)?)
}

/// This device's backups.
#[tauri::command]
pub async fn backups_get(state: State<'_, AppState>) -> CommandResult<BackupsView> {
    view(&state)
}

/// Sets backups up: a new repository at the target (`create`), or one
/// there already, opened with its password. A secret left blank keeps the
/// one set before. Daily, 7 daily, 4 weekly, 12 monthly unless set before.
#[tauri::command]
pub async fn backups_connect(
    app: AppHandle,
    target: TargetConfig,
    password: String,
    create: bool,
    host: String,
) -> CommandResult<BackupsView> {
    blocking(app, move |state| {
        let old = state.quietly(|s| s.store.backup_config())?;
        let target = match &old {
            Some(old) => target.keeping_secrets_of(&old.target),
            None => target,
        };
        let opened = target.open()?;
        if create {
            Repo::init(opened, &password, KdfParams::DEFAULT, now())
                .map_err(app_core::Error::from)?;
        } else {
            Repo::open(opened, &password).map_err(app_core::Error::from)?;
        }
        let config = BackupConfig {
            target,
            password,
            schedule: old.as_ref().map_or(Schedule::Daily, |o| o.schedule),
            policy: old.as_ref().map_or_else(Policy::default, |o| o.policy),
            host: if host.trim().is_empty() {
                crate::sync::sign_in::device().name
            } else {
                host.trim().to_owned()
            },
        };
        state.quietly(|s| {
            s.store.set_backup_config(Some(&config))?;
            // a new place: its history starts now
            s.store.set_backup_status(&BackupStatus::default())
        })?;
        view(state)
    })
    .await
}

/// When to run and what to keep.
#[tauri::command]
pub async fn backups_settings(
    state: State<'_, AppState>,
    schedule: Schedule,
    policy: Policy,
) -> CommandResult<BackupsView> {
    let mut config = config(&state)?;
    config.schedule = schedule;
    config.policy = policy;
    state.quietly(|s| s.store.set_backup_config(Some(&config)))?;
    view(&state)
}

/// Stops backing up here. The repository stays where it is.
#[tauri::command]
pub async fn backups_disconnect(state: State<'_, AppState>) -> CommandResult<BackupsView> {
    state.quietly(|s| s.store.set_backup_config(None))?;
    view(&state)
}

fn run(state: &AppState, offset_seconds: i64) -> CommandResult<()> {
    let config = config(state)?;
    let (data, blobs) = state.quietly(|s| Ok((s.store.backup_data()?, s.blobs.clone())))?;
    let repo = open(&config)?;
    let report = app_core::repo::backup_data(&repo, &data, &blobs, &config.host, &now)?;
    let snapshots: Vec<(String, i64)> = repo
        .snapshots()
        .map_err(app_core::Error::from)?
        .into_iter()
        // each device keeps its own history, another's is its own to thin
        .filter(|s| s.host == config.host)
        .map(|s| (s.id, s.time))
        .collect();
    let kept = app_core::repo::keep(&snapshots, config.policy, offset_seconds);
    let gone: std::collections::HashSet<String> = snapshots
        .into_iter()
        .map(|(id, _)| id)
        .filter(|id| !kept.contains(id))
        .collect();
    if !gone.is_empty() {
        repo.forget(&gone).map_err(app_core::Error::from)?;
        repo.prune(&config.host, now())
            .map_err(app_core::Error::from)?;
    }
    let count = repo.snapshots().map_err(app_core::Error::from)?.len() as u64;
    let stored: u64 = repo
        .target()
        .list("data/")
        .map_err(app_core::Error::from)?
        .iter()
        .map(|l| l.size)
        .sum();
    state.quietly(|s| {
        let mut status = s.store.backup_status()?;
        status.last_success_at = Some(now());
        status.last_error = None;
        status.last_error_kind = None;
        status.last_report = Some(report);
        status.snapshots = count;
        status.stored_bytes = stored;
        s.store.set_backup_status(&status)
    })?;
    Ok(())
}

/// Backs up now: a snapshot, then what the policy no longer keeps of this
/// device's goes. `offset_seconds`: the time zone, for days and weeks.
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

/// The repository's snapshots, newest first.
#[tauri::command]
pub async fn backups_snapshots(app: AppHandle) -> CommandResult<Vec<SnapshotView>> {
    blocking(app, |state| {
        let repo = open(&config(state)?)?;
        let mut out: Vec<SnapshotView> = repo
            .snapshots()
            .map_err(app_core::Error::from)?
            .into_iter()
            .filter(|s| s.kind == "vault")
            .map(|s| SnapshotView {
                size: s.size(),
                id: s.id,
                time: s.time,
                host: s.host,
                app_version: s.app_version,
                counts: s.counts,
            })
            .collect();
        out.reverse();
        Ok(out)
    })
    .await
}

/// Puts a snapshot back: what is missing here comes back with its ids,
/// what is here stays as it is.
#[tauri::command]
pub async fn backups_restore(app: AppHandle, snapshot: String) -> CommandResult<RestoreReport> {
    blocking(app, move |state| {
        let repo = open(&config(state)?)?;
        let contents = app_core::repo::read_snapshot(&repo, &snapshot)?;
        let (present, blobs) = state.quietly(|s| {
            Ok((
                s.store.notes_present(&contents.note_ids())?,
                s.blobs.clone(),
            ))
        })?;
        app_core::repo::fetch_files(&repo, &contents, &present, &blobs)?;
        state.quietly(|s| {
            let has = |sha: &str| s.blobs.has(sha).unwrap_or(false);
            s.store.restore_backup(&contents.data, &has)
        })
    })
    .await
}

/// Restores a snapshot into a scratch vault, compares, removes it.
#[tauri::command]
pub async fn backups_test_restore(app: AppHandle, snapshot: String) -> CommandResult<TestRestore> {
    blocking(app, move |state| {
        let repo = open(&config(state)?)?;
        let scratch = state.dir.join(format!("backup-test-{}", now()));
        Ok(app_core::repo::test_restore(&repo, &snapshot, &scratch)?)
    })
    .await
}

/// Checks the repository, reading every pack when `full`.
#[tauri::command]
pub async fn backups_check(app: AppHandle, full: bool) -> CommandResult<CheckReport> {
    blocking(app, move |state| {
        let repo = open(&config(state)?)?;
        Ok(repo.check(full).map_err(app_core::Error::from)?)
    })
    .await
}
