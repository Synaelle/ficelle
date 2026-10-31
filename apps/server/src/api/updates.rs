// SPDX-License-Identifier: AGPL-3.0-or-later

//! The dashboard's update notice and security checklist.

use std::collections::BTreeMap;

use app_protocol::account::Registration;
use app_protocol::admin::{CheckState, Checklist, ChecklistItem};
use app_protocol::releases::{UpdatesRequest, UpdatesView};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;

use super::admin::admin;
use super::extract::{Body, ClientIp, CurrentUser, Secure};
use crate::accounts::factors;
use crate::accounts::store::{self, Event};
use crate::db::DbError;
use crate::error::Error;
use crate::routes::AppState;
use crate::updates;

/// `GET /api/v1/admin/updates`
pub async fn get(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
) -> Result<Json<UpdatesView>, Error> {
    admin(&me)?;
    let enabled = state.db.call(|c| updates::enabled(c)).await?;
    Ok(Json(state.updates.view(enabled).await))
}

/// `PUT /api/v1/admin/updates`: the daily check on or off.
pub async fn set(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(req): Body<UpdatesRequest>,
) -> Result<Json<UpdatesView>, Error> {
    admin(&me)?;
    let now = state.clock.now();
    let ip = ip.0.to_string();
    let id = me.user.id.clone();
    state
        .db
        .call(move |c| {
            updates::set_enabled(c, req.enabled)?;
            store::audit(
                c,
                now,
                &Event {
                    name: "updates_check",
                    user_id: Some(&id),
                    ip: Some(&ip),
                    detail: Some(if req.enabled { "on" } else { "off" }),
                    ..Event::default()
                },
            )
        })
        .await?;
    if req.enabled && state.updates.available() && state.updates.due(now).await {
        state.updates.check(now).await;
    }
    Ok(Json(state.updates.view(req.enabled).await))
}

/// `POST /api/v1/admin/updates/check`: asks the release list now.
pub async fn check(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
) -> Result<Json<UpdatesView>, Error> {
    admin(&me)?;
    let enabled = state.db.call(|c| updates::enabled(c)).await?;
    if !enabled || !state.updates.available() {
        return Err(Error::new(
            StatusCode::CONFLICT,
            app_protocol::error_code::INVALID,
            "the update check is off",
        ));
    }
    state.updates.check(state.clock.now()).await;
    Ok(Json(state.updates.view(true).await))
}

/// What the checklist reads from the database at once.
struct Facts {
    admins_without: Vec<String>,
    registration: Registration,
    backups: Option<crate::backups::BackupConfig>,
    status: crate::backups::BackupStatus,
    updates: bool,
    secrets_dismissed: bool,
    /// Storage per person for someone with no rule of their own, 0 for none.
    quota_bytes: u64,
}

fn facts(
    conn: &Connection,
    files: &crate::config::FilesConfig,
    master: &crate::keys::MasterKey,
) -> Result<Facts, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, username FROM users
         WHERE role = 'admin' AND disabled_at IS NULL ORDER BY username",
    )?;
    let admins = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut admins_without = Vec::new();
    for (id, name) in admins {
        if !factors::has_second_factor(conn, &id)? {
            admins_without.push(name);
        }
    }
    let dismissed: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'secrets_reminder'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(Facts {
        admins_without,
        registration: store::registration(conn)?,
        backups: crate::backups::config(conn, master)?,
        status: crate::backups::status(conn)?,
        updates: updates::enabled(conn)?,
        secrets_dismissed: dismissed.is_some(),
        quota_bytes: crate::files::policy::everyone(conn, files)?.quota_bytes,
    })
}

fn item(key: &str, state: CheckState, values: &[(&str, String)]) -> ChecklistItem {
    ChecklistItem {
        key: key.into(),
        state,
        values: values
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect::<BTreeMap<_, _>>(),
        dismissed: false,
    }
}

/// A line as it stands now, to remember it hidden: once it says something
/// else (another admin without a factor, a newer version), it shows again.
fn fingerprint(item: &ChecklistItem) -> String {
    let values: Vec<String> = item
        .values
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    format!("{}|{:?}|{}", item.key, item.state, values.join("&"))
}

/// The lines an admin hid, as they stood then.
fn hidden(conn: &Connection) -> Result<Vec<String>, DbError> {
    let text: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'checklist_hidden'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(text
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default())
}

fn set_hidden(conn: &Connection, lines: &[String]) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('checklist_hidden', ?1)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        [serde_json::to_string(lines).unwrap_or_default()],
    )?;
    Ok(())
}

/// The backups line: none set, failing, late, or fine.
fn backups_item(facts: &Facts, now: i64) -> ChecklistItem {
    let status = &facts.status;
    let at = |t: Option<i64>| t.map(|t| t.to_string()).unwrap_or_default();
    let Some(config) = &facts.backups else {
        return item("backups", CheckState::Warn, &[("state", "none".into())]);
    };
    let failed = status.last_error.is_some()
        && status.last_run_at.unwrap_or(0) > status.last_success_at.unwrap_or(0);
    if failed {
        return item(
            "backups",
            CheckState::Fail,
            &[
                ("state", "failing".into()),
                ("error", status.last_error.clone().unwrap_or_default()),
            ],
        );
    }
    let Some(success) = status.last_success_at else {
        return item("backups", CheckState::Warn, &[("state", "never".into())]);
    };
    // a run may be a little late, two periods (at least a day) is too late
    let late = config
        .schedule
        .period_ms()
        .is_some_and(|p| now - success > (2 * p).max(crate::clock::DAY));
    let manual = config.schedule.period_ms().is_none();
    let state = if late || manual {
        CheckState::Warn
    } else {
        CheckState::Ok
    };
    let word = if late {
        "late"
    } else if manual {
        "manual"
    } else {
        "fine"
    };
    item(
        "backups",
        state,
        &[("state", word.into()), ("at", at(Some(success)))],
    )
}

/// The version line: the check off, a security release, a newer one, or up
/// to date.
fn updates_item(view: UpdatesView) -> ChecklistItem {
    let version = [("current", view.current.clone())];
    if !view.available || !view.enabled {
        item("updates", CheckState::Warn, &[("state", "off".into())])
    } else if view.security {
        let latest = view.latest.map(|r| r.version).unwrap_or_default();
        item(
            "updates",
            CheckState::Fail,
            &[
                ("state", "security".into()),
                ("latest", latest),
                version[0].clone(),
            ],
        )
    } else if view.newer {
        let latest = view.latest.map(|r| r.version).unwrap_or_default();
        item(
            "updates",
            CheckState::Warn,
            &[
                ("state", "newer".into()),
                ("latest", latest),
                version[0].clone(),
            ],
        )
    } else if view.checked_at.is_none() {
        item(
            "updates",
            CheckState::Ok,
            &[("state", "waiting".into()), version[0].clone()],
        )
    } else {
        item(
            "updates",
            CheckState::Ok,
            &[("state", "current".into()), version[0].clone()],
        )
    }
}

/// `GET /api/v1/admin/checklist`
pub async fn checklist(
    State(state): State<AppState>,
    secure: Secure,
    CurrentUser(me): CurrentUser,
) -> Result<Json<Checklist>, Error> {
    admin(&me)?;
    let mut items = lines(&state, &secure).await?;
    let hidden = state.db.call(|c| hidden(c)).await?;
    for item in &mut items {
        item.dismissed = item.state != CheckState::Ok && hidden.contains(&fingerprint(item));
    }
    Ok(Json(Checklist { items }))
}

/// The checklist's lines, as things stand.
async fn lines(state: &AppState, secure: &Secure) -> Result<Vec<ChecklistItem>, Error> {
    let files = state.config.files.clone();
    let master = state.master.clone();
    let facts = state.db.call(move |c| facts(c, &files, &master)).await?;
    let now = state.clock.now();
    let mut items = Vec::new();
    items.push(if facts.admins_without.is_empty() {
        item("secondFactors", CheckState::Ok, &[])
    } else {
        item(
            "secondFactors",
            CheckState::Fail,
            &[("names", facts.admins_without.join(", "))],
        )
    });
    items.push(match facts.registration {
        Registration::Open => item("registration", CheckState::Warn, &[("mode", "open".into())]),
        Registration::Invite => item("registration", CheckState::Ok, &[("mode", "invite".into())]),
        Registration::Closed => item("registration", CheckState::Ok, &[("mode", "closed".into())]),
    });
    items.push(if facts.quota_bytes == 0 {
        item(
            "storage",
            CheckState::Warn,
            &[("state", "unlimited".into())],
        )
    } else {
        item(
            "storage",
            CheckState::Ok,
            &[("mb", (facts.quota_bytes / (1024 * 1024)).to_string())],
        )
    });
    items.push(backups_item(&facts, now));
    items.push(updates_item(state.updates.view(facts.updates).await));
    let url = &state.config.server.public_url;
    items.push(if secure.0 && url.starts_with("https://") {
        item("tls", CheckState::Ok, &[])
    } else if secure.0 {
        item("tls", CheckState::Warn, &[("state", "local".into())])
    } else {
        item("tls", CheckState::Fail, &[("state", "plain".into())])
    });
    items.push(if url.is_empty() {
        item("address", CheckState::Fail, &[("state", "unset".into())])
    } else if url.starts_with("https://") {
        item("address", CheckState::Ok, &[("url", url.clone())])
    } else {
        item(
            "address",
            CheckState::Warn,
            &[("state", "http".into()), ("url", url.clone())],
        )
    });
    if !facts.secrets_dismissed {
        let apart = !state.config.storage.master_key_file.as_os_str().is_empty();
        items.push(item(
            "secrets",
            CheckState::Warn,
            &[("apart", apart.to_string())],
        ));
    }
    Ok(items)
}

/// `POST /api/v1/admin/checklist/dismiss`
#[derive(Deserialize)]
pub struct Dismiss {
    key: String,
}

/// Hides a line an admin finds fine as it is, as it stands now. The
/// reminder about the secrets goes for good once done.
pub async fn dismiss(
    State(state): State<AppState>,
    secure: Secure,
    CurrentUser(me): CurrentUser,
    Body(req): Body<Dismiss>,
) -> Result<StatusCode, Error> {
    admin(&me)?;
    if req.key != "secrets" {
        let items = lines(&state, &secure).await?;
        let line = items
            .iter()
            .find(|i| i.key == req.key && i.state != CheckState::Ok)
            .map(fingerprint)
            .ok_or_else(|| Error::invalid("no such line to hide"))?;
        // the lines that no longer stand as they did are forgotten
        let current: Vec<String> = items.iter().map(fingerprint).collect();
        state
            .db
            .call(move |c| {
                let mut kept: Vec<String> = hidden(c)?
                    .into_iter()
                    .filter(|h| current.contains(h) && *h != line)
                    .collect();
                kept.push(line);
                set_hidden(c, &kept)
            })
            .await?;
        return Ok(StatusCode::NO_CONTENT);
    }
    state
        .db
        .call(|c| {
            c.execute(
                "INSERT INTO settings (key, value) VALUES ('secrets_reminder', 'dismissed')
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                [],
            )?;
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/v1/admin/checklist/restore`: every hidden line shows again.
pub async fn restore(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
) -> Result<StatusCode, Error> {
    admin(&me)?;
    state.db.call(|c| set_hidden(c, &[])).await?;
    Ok(StatusCode::NO_CONTENT)
}
