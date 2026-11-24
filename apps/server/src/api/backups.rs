// SPDX-License-Identifier: AGPL-3.0-or-later

//! The instance's backups on the admin page (`/admin/backups`): set up,
//! how many to keep, run, list, check and test restore. A whole instance
//! is restored from the command line, the server stopped.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use super::admin::{admin, admin_event};
use super::extract::{Body, ClientIp, CurrentUser};
use crate::backups::{
    self, BackupConfig, BackupStatus, Schedule, ServerTarget, instance::TestRestore,
};
use crate::db::DbError;
use crate::error::Error;
use crate::routes::AppState;
use app_repo::CheckReport;

fn failed(why: impl Into<String>) -> Error {
    Error::new(
        StatusCode::BAD_GATEWAY,
        app_protocol::error_code::BACKUP_FAILED,
        why,
    )
}

/// The settings as the page sees them: no password.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigView {
    target: ServerTarget,
    /// How many backups stay. For older settings, which keep their own
    /// schedule and policy until a number is chosen, how many there are:
    /// choosing it as it is removes none.
    keep: u32,
}

/// `GET /api/v1/admin/backups`
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupsView {
    config: Option<ConfigView>,
    status: BackupStatus,
    running: bool,
    /// The master key is read from a file of its own (`storage.master_key_file`).
    master_key_apart: bool,
    /// Where the simple setup puts backups: `storage.backups_dir` when the
    /// server can write there (the image's /backups volume), else a folder
    /// in the data folder, the one place a container can always write.
    suggested_folder: String,
    /// The suggested folder is not inside the data folder.
    suggested_apart: bool,
}

/// `storage.backups_dir` when a file can be made in it, else `backups` in
/// the data folder; and whether it lies outside the data folder.
fn suggested_folder(storage: &crate::config::StorageConfig) -> (std::path::PathBuf, bool) {
    let dir = &storage.backups_dir;
    if !dir.as_os_str().is_empty() {
        let probe = dir.join(".write-check");
        if std::fs::write(&probe, b"").is_ok() {
            let _ = std::fs::remove_file(&probe);
            return (dir.clone(), !dir.starts_with(&storage.data_dir));
        }
    }
    (storage.data_dir.join("backups"), false)
}

async fn view(state: &AppState) -> Result<BackupsView, Error> {
    let master = state.master.clone();
    let (config, status) = state
        .db
        .call(move |c| Ok((backups::config(c, &master)?, backups::status(c)?)))
        .await?;
    let (suggested, suggested_apart) = suggested_folder(&state.config.storage);
    let there = u32::try_from(status.snapshots).unwrap_or(backups::MAX_KEEP);
    Ok(BackupsView {
        config: config.map(|c| ConfigView {
            keep: c
                .keep()
                .unwrap_or_else(|| there.clamp(1, backups::MAX_KEEP)),
            target: c.target,
        }),
        status,
        running: backups::running(),
        master_key_apart: !state.config.storage.master_key_file.as_os_str().is_empty(),
        suggested_folder: suggested.display().to_string(),
        suggested_apart,
    })
}

/// `GET /api/v1/admin/backups`
pub async fn get(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
) -> Result<Json<BackupsView>, Error> {
    admin(&me)?;
    Ok(Json(view(&state).await?))
}

/// `PUT /api/v1/admin/backups`: a new repository at the target, or one
/// there opened with its password.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connect {
    target: ServerTarget,
    password: String,
    create: bool,
    #[serde(default)]
    offset_seconds: i64,
}

/// `PUT /api/v1/admin/backups`
pub async fn connect(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(connect): Body<Connect>,
) -> Result<Json<BackupsView>, Error> {
    admin(&me)?;
    if connect.password.is_empty() {
        return Err(Error::invalid("a backup needs a password"));
    }
    let master = state.master.clone();
    let old = match state.db.call(move |c| backups::config(c, &master)).await {
        Ok(old) => old,
        // settings that no longer read back are what this replaces
        Err(DbError::Data(_)) => None,
        Err(e) => return Err(e.into()),
    };
    let config = BackupConfig {
        target: connect.target,
        password: connect.password,
        schedule: old.as_ref().map_or(Schedule::Daily, |o| o.schedule),
        policy: old
            .as_ref()
            .map_or_else(|| backups::keeping(backups::DEFAULT_KEEP), |o| o.policy),
        offset_seconds: connect.offset_seconds,
    };
    let now = state.clock.now();
    let checking = config.clone();
    tokio::task::spawn_blocking(move || backups::open(&checking, connect.create, now).map(|_| ()))
        .await
        .map_err(|e| Error::internal(&e.to_string()))?
        .map_err(failed)?;
    let ip_text = ip.0.to_string();
    let master = state.master.clone();
    state
        .db
        .call(move |c| {
            backups::set_config(c, &master, Some(&config))?;
            backups::set_status(c, &BackupStatus::default())?;
            admin_event(c, now, "backup_set_up", None, &me, &ip_text, "")
        })
        .await?;
    Ok(Json(view(&state).await?))
}

/// `PUT /api/v1/admin/backups/settings`: how many backups stay, made
/// daily.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    keep: u32,
    #[serde(default)]
    offset_seconds: i64,
}

/// `PUT /api/v1/admin/backups/settings`
pub async fn settings(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
    Body(settings): Body<Settings>,
) -> Result<Json<BackupsView>, Error> {
    admin(&me)?;
    if !(1..=backups::MAX_KEEP).contains(&settings.keep) {
        return Err(Error::invalid(format!(
            "keep 1 to {} backups",
            backups::MAX_KEEP
        )));
    }
    let master = state.master.clone();
    state
        .db
        .call(move |c| {
            let Some(mut config) = backups::config(c, &master)? else {
                return Ok(false);
            };
            config.schedule = Schedule::Daily;
            config.policy = backups::keeping(settings.keep);
            config.offset_seconds = settings.offset_seconds;
            backups::set_config(c, &master, Some(&config))?;
            Ok(true)
        })
        .await?
        .then_some(())
        .ok_or_else(Error::not_found)?;
    Ok(Json(view(&state).await?))
}

/// `DELETE /api/v1/admin/backups`: stops backing up, the backups stay.
pub async fn stop(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
) -> Result<Json<BackupsView>, Error> {
    admin(&me)?;
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    let master = state.master.clone();
    state
        .db
        .call(move |c| {
            backups::set_config(c, &master, None)?;
            admin_event(c, now, "backup_stopped", None, &me, &ip_text, "")
        })
        .await?;
    Ok(Json(view(&state).await?))
}

/// `POST /api/v1/admin/backups/run`
pub async fn run(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
) -> Result<Json<BackupsView>, Error> {
    admin(&me)?;
    backups::run(&state, &me.user.username)
        .await
        .map_err(failed)?;
    Ok(Json(view(&state).await?))
}

async fn opened(state: &AppState) -> Result<app_repo::Repo, Error> {
    let master = state.master.clone();
    let config = state
        .db
        .call(move |c| backups::config(c, &master))
        .await?
        .ok_or_else(Error::not_found)?;
    let now = state.clock.now();
    tokio::task::spawn_blocking(move || backups::open(&config, false, now))
        .await
        .map_err(|e| Error::internal(&e.to_string()))?
        .map_err(failed)
}

/// A snapshot as listed.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotView {
    id: String,
    time: i64,
    app_version: String,
    counts: std::collections::BTreeMap<String, u64>,
    size: u64,
}

/// `GET /api/v1/admin/backups/snapshots`, newest first.
pub async fn snapshots(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
) -> Result<Json<Vec<SnapshotView>>, Error> {
    admin(&me)?;
    let repo = opened(&state).await?;
    let mut list: Vec<SnapshotView> = tokio::task::spawn_blocking(move || repo.snapshots())
        .await
        .map_err(|e| Error::internal(&e.to_string()))?
        .map_err(|e| failed(e.to_string()))?
        .into_iter()
        .filter(|s| s.kind == "server")
        .map(|s| SnapshotView {
            size: s.size(),
            id: s.id,
            time: s.time,
            app_version: s.app_version,
            counts: s.counts,
        })
        .collect();
    list.reverse();
    Ok(Json(list))
}

/// `POST /api/v1/admin/backups/check`
#[derive(Deserialize)]
pub struct Check {
    full: bool,
}

/// `POST /api/v1/admin/backups/check`
pub async fn check(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
    Body(check): Body<Check>,
) -> Result<Json<CheckReport>, Error> {
    admin(&me)?;
    let repo = opened(&state).await?;
    Ok(Json(
        tokio::task::spawn_blocking(move || repo.check(check.full))
            .await
            .map_err(|e| Error::internal(&e.to_string()))?
            .map_err(|e| failed(e.to_string()))?,
    ))
}

/// One snapshot.
#[derive(Deserialize)]
pub struct Pick {
    snapshot: String,
}

/// `POST /api/v1/admin/backups/test-restore`: into a scratch folder,
/// opened, compared, removed.
pub async fn test_restore(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
    Body(pick): Body<Pick>,
) -> Result<Json<TestRestore>, Error> {
    admin(&me)?;
    let repo = opened(&state).await?;
    let scratch = backups::scratch(&state, "test");
    Ok(Json(
        tokio::task::spawn_blocking(move || {
            backups::instance::test_restore(&repo, &pick.snapshot, &scratch)
        })
        .await
        .map_err(|e| Error::internal(&e.to_string()))?
        .map_err(failed)?,
    ))
}
