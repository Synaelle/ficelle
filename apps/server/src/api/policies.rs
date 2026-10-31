// SPDX-License-Identifier: AGPL-3.0-or-later

//! The admin's files policies, for the instance, a group or a person, and
//! the storage the instance and each person use. See
//! [`crate::files::policy`] for how the levels combine.

use app_protocol::admin::{PolicySettings, PolicyView, StorageOverview};
use app_protocol::error_code;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use rusqlite::Connection;
use serde::Deserialize;

use super::admin::{admin, admin_event};
use super::extract::{Body, ClientIp, CurrentUser};
use crate::admin as admin_store;
use crate::db::DbError;
use crate::error::Error;
use crate::files::policy::{self, Levels, Scope};
use crate::routes::AppState;

/// Paging for the storage list.
#[derive(Debug, Deserialize)]
pub struct StorageQuery {
    #[serde(default)]
    offset: u32,
    #[serde(default)]
    limit: Option<u32>,
}

/// `GET /api/v1/admin/storage?offset=&limit=`
pub async fn storage(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
    Query(q): Query<StorageQuery>,
) -> Result<Json<StorageOverview>, Error> {
    admin(&me)?;
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let config = state.config.clone();
    Ok(Json(
        state
            .db
            .call(move |c| crate::files::storage(c, &config.files, q.offset, limit))
            .await?,
    ))
}

fn not_found(what: &str) -> Error {
    Error::new(
        StatusCode::NOT_FOUND,
        error_code::NOT_FOUND,
        format!("no such {what}"),
    )
}

/// The levels a view shows, `None` when the group or person is gone.
fn levels(conn: &Connection, scope: Scope, target: &str) -> Result<Option<Levels>, DbError> {
    let instance = policy::settings(conn, Scope::Instance, "")?;
    Ok(match scope {
        Scope::Instance => Some(Levels {
            instance,
            ..Levels::default()
        }),
        // what the group's members get unless set for them otherwise
        Scope::Group => match admin_store::group(conn, target)? {
            Some(group) => Some(Levels {
                user: None,
                groups: vec![(group.name, policy::settings(conn, Scope::Group, target)?)],
                instance,
            }),
            None => None,
        },
        Scope::User => match admin_store::admin_user(conn, target)? {
            Some(_) => Some(policy::levels_of(conn, target)?),
            None => None,
        },
    })
}

async fn view(state: &AppState, scope: Scope, target: String) -> Result<Json<PolicyView>, Error> {
    let config = state.config.clone();
    let found = state
        .db
        .call(move |c| {
            let Some(levels) = levels(c, scope, &target)? else {
                return Ok(None);
            };
            let settings = policy::settings(c, scope, &target)?;
            let (effective, sources) = policy::resolve(&config.files, &levels);
            Ok(Some(PolicyView {
                settings,
                effective,
                sources,
            }))
        })
        .await?;
    found.map(Json).ok_or_else(|| {
        not_found(if scope == Scope::Group {
            "group"
        } else {
            "account"
        })
    })
}

async fn change(
    state: &AppState,
    ip: &ClientIp,
    me: crate::accounts::store::Authed,
    scope: Scope,
    target: String,
    settings: PolicySettings,
) -> Result<Json<PolicyView>, Error> {
    admin(&me)?;
    if let Some(why) = policy::invalid(&settings) {
        return Err(Error::invalid(why));
    }
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    let t = target.clone();
    let found = state
        .db
        .call(move |c| {
            if levels(c, scope, &t)?.is_none() {
                return Ok(false);
            }
            let tx = c.transaction()?;
            policy::set(&tx, scope, &t, &settings, now)?;
            let (subject, what) = match scope {
                Scope::Instance => (None, "instance".to_owned()),
                Scope::Group => (None, format!("group {t}")),
                Scope::User => (Some(t.as_str()), "person".to_owned()),
            };
            admin_event(&tx, now, "policy_changed", subject, &me, &ip_text, &what)?;
            tx.commit()?;
            Ok(true)
        })
        .await?;
    if !found {
        return Err(not_found(if scope == Scope::Group {
            "group"
        } else {
            "account"
        }));
    }
    view(state, scope, target).await
}

/// `GET /api/v1/admin/policy`
pub async fn instance(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
) -> Result<Json<PolicyView>, Error> {
    admin(&me)?;
    view(&state, Scope::Instance, String::new()).await
}

/// `PUT /api/v1/admin/policy`
pub async fn set_instance(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(settings): Body<PolicySettings>,
) -> Result<Json<PolicyView>, Error> {
    change(&state, &ip, me, Scope::Instance, String::new(), settings).await
}

/// `GET /api/v1/admin/groups/{id}/policy`
pub async fn group(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
    Path(id): Path<String>,
) -> Result<Json<PolicyView>, Error> {
    admin(&me)?;
    view(&state, Scope::Group, id).await
}

/// `PUT /api/v1/admin/groups/{id}/policy`
pub async fn set_group(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Path(id): Path<String>,
    Body(settings): Body<PolicySettings>,
) -> Result<Json<PolicyView>, Error> {
    change(&state, &ip, me, Scope::Group, id, settings).await
}

/// `GET /api/v1/admin/users/{id}/policy`: what applies to the person.
pub async fn user(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
    Path(id): Path<String>,
) -> Result<Json<PolicyView>, Error> {
    admin(&me)?;
    view(&state, Scope::User, id).await
}

/// `PUT /api/v1/admin/users/{id}/policy`
pub async fn set_user(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Path(id): Path<String>,
    Body(settings): Body<PolicySettings>,
) -> Result<Json<PolicyView>, Error> {
    change(&state, &ip, me, Scope::User, id, settings).await
}
