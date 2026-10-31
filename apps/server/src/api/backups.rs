// SPDX-License-Identifier: AGPL-3.0-or-later

//! The instance's backups on the admin page (`/admin/backups`): set up,
//! run, list, check, test restore, and bring back one person's data. A
//! whole instance is restored from the command line, the server stopped.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use super::admin::{admin, admin_event};
use super::extract::{Body, ClientIp, CurrentUser};
use crate::backups::{
    self, BackupConfig, BackupStatus, Schedule, ServerTarget,
    instance::{PersonReport, TestRestore},
};
use crate::error::Error;
use crate::routes::AppState;
use app_repo::{CheckReport, Policy};

fn failed(why: impl Into<String>) -> Error {
    Error::new(
        StatusCode::BAD_GATEWAY,
        app_protocol::error_code::BACKUP_FAILED,
        why,
    )
}

/// The settings as the page sees them: no secrets.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigView {
    target: ServerTarget,
    schedule: Schedule,
    policy: Policy,
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
    /// Where the simple setup puts backups: a folder in the data folder,
    /// the one place a container can always write.
    suggested_folder: String,
}

async fn view(state: &AppState) -> Result<BackupsView, Error> {
    let master = state.master.clone();
    let (config, status) = state
        .db
        .call(move |c| Ok((backups::config(c, &master)?, backups::status(c)?)))
        .await?;
    Ok(BackupsView {
        config: config.map(|c| ConfigView {
            target: c.target.without_secrets(),
            schedule: c.schedule,
            policy: c.policy,
        }),
        status,
        running: backups::running(),
        master_key_apart: !state.config.storage.master_key_file.as_os_str().is_empty(),
        suggested_folder: state
            .config
            .storage
            .data_dir
            .join("backups")
            .display()
            .to_string(),
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
    let old = state.db.call(move |c| backups::config(c, &master)).await?;
    let target = match &old {
        Some(old) => connect.target.keeping_secrets_of(&old.target),
        None => connect.target,
    };
    let config = BackupConfig {
        target,
        password: connect.password,
        schedule: old.as_ref().map_or(Schedule::Daily, |o| o.schedule),
        policy: old.as_ref().map_or_else(Policy::default, |o| o.policy),
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

/// `PUT /api/v1/admin/backups/settings`
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    schedule: Schedule,
    policy: Policy,
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
    let master = state.master.clone();
    state
        .db
        .call(move |c| {
            let Some(mut config) = backups::config(c, &master)? else {
                return Ok(false);
            };
            config.schedule = settings.schedule;
            config.policy = settings.policy;
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

/// `POST /api/v1/admin/backups/restore-person`
#[derive(Deserialize)]
pub struct Person {
    snapshot: String,
    username: String,
}

/// `POST /api/v1/admin/backups/restore-person`: what is missing of a
/// person comes back, nothing here changes.
pub async fn restore_person(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(person): Body<Person>,
) -> Result<Json<PersonReport>, Error> {
    admin(&me)?;
    let repo = std::sync::Arc::new(opened(&state).await?);
    let scratch = backups::scratch(&state, "person");
    let (r, s, snap) = (repo.clone(), scratch.clone(), person.snapshot.clone());
    let database = tokio::task::spawn_blocking(move || {
        std::fs::create_dir_all(&s).map_err(|e| e.to_string())?;
        backups::instance::snapshot_database(&r, &snap, &s)
    })
    .await
    .map_err(|e| Error::internal(&e.to_string()))?;
    let result = async {
        let database = database.map_err(failed)?;
        let (db, name) = (database.clone(), person.username.clone());
        let (user, objects) = state
            .db
            .call(move |c| Ok(backups::instance::person_plan(c, &db, &name)))
            .await?
            .map_err(Error::invalid)?;
        // the files first: the rows that name them come after
        let files = state.files.clone();
        let (r, snap) = (repo.clone(), person.snapshot.clone());
        let restored_files = tokio::task::spawn_blocking(move || {
            backups::instance::restore_objects(&r, &snap, &objects, files.as_ref())
        })
        .await
        .map_err(|e| Error::internal(&e.to_string()))?
        .map_err(failed)?;
        let now = state.clock.now();
        let ip_text = ip.0.to_string();
        let username = person.username.clone();
        let mut report = state
            .db
            .call(move |c| {
                let report = backups::instance::restore_person(c, &database, &user);
                if report.is_ok() {
                    admin_event(
                        c,
                        now,
                        "backup_person_restored",
                        Some(&user),
                        &me,
                        &ip_text,
                        &username,
                    )?;
                }
                Ok(report)
            })
            .await?
            .map_err(failed)?;
        report.files = restored_files;
        Ok::<_, Error>(report)
    }
    .await;
    if scratch.exists() {
        let _ = std::fs::remove_dir_all(&scratch);
    }
    Ok(Json(result?))
}
