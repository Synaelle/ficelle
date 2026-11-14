// SPDX-License-Identifier: AGPL-3.0-or-later

//! The web version on the admin page: who may open it and the sign-in
//! limits, and the cookie a browser gets once signed in there.

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use super::admin::{admin, admin_event};
use super::extract::{Body, ClientIp, CurrentUser};
use crate::accounts::store::{self, Limits};
use crate::clock::DAY;
use crate::error::Error;
use crate::routes::AppState;
use crate::webapp::{self, Access};

/// What the admin page shows of the web version.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebView {
    access: Access,
    /// The web build is on this server.
    files: bool,
    /// Where people open it, empty without `server.public_url`.
    address: String,
    /// The sign-in limits set on the admin page.
    limits: Limits,
    /// The config file's limits, used where none is set.
    defaults: Limits,
    /// Sign-ins and refused ones in the last day, every way of signing in.
    sign_ins: i64,
    refused: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccessRequest {
    access: Access,
}

fn defaults(state: &AppState) -> Limits {
    let auth = &state.config.auth;
    Limits {
        per_minute: Some(auth.rate_limit_per_minute),
        attempts: Some(auth.lockout_attempts),
        first_wait: Some(auth.lockout_minutes),
        longest_wait: Some(auth.lockout_max_minutes),
    }
}

async fn view(state: &AppState) -> Result<WebView, Error> {
    let since = state.clock.now() - DAY;
    let (access, limits, sign_ins, refused) = state
        .db
        .call(move |c| {
            let count = |event: &str| -> Result<i64, crate::db::DbError> {
                Ok(c.query_row(
                    "SELECT count(*) FROM audit_log WHERE event = ?1 AND at >= ?2",
                    rusqlite::params![event, since],
                    |r| r.get(0),
                )?)
            };
            Ok((
                webapp::access(c)?,
                store::limits(c)?,
                count("sign_in")?,
                count("sign_in_failed")?,
            ))
        })
        .await?;
    let public = state.config.server.public_url.trim_end_matches('/');
    Ok(WebView {
        access,
        files: webapp::folder(state).is_some(),
        address: if public.is_empty() {
            String::new()
        } else {
            format!("{public}/web/")
        },
        limits,
        defaults: defaults(state),
        sign_ins,
        refused,
    })
}

/// `GET /api/v1/admin/web`
pub async fn get(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
) -> Result<Json<WebView>, Error> {
    admin(&me)?;
    Ok(Json(view(&state).await?))
}

/// `PUT /api/v1/admin/web`: who may open it, `off` to stop serving it.
pub async fn set(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(req): Body<AccessRequest>,
) -> Result<Json<WebView>, Error> {
    admin(&me)?;
    if req.access != Access::Off && webapp::folder(&state).is_none() {
        return Err(Error::invalid(
            "the web version's files are not on this server",
        ));
    }
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    let access = req.access;
    state
        .db
        .call(move |c| {
            webapp::set_access(c, access)?;
            let what = match access {
                Access::Off => "off",
                Access::SignedIn => "signed-in people",
                Access::Anyone => "anyone",
            };
            admin_event(c, now, "web_version_changed", None, &me, &ip_text, what)
        })
        .await?;
    Ok(Json(view(&state).await?))
}

/// `PUT /api/v1/admin/web/limits`: the sign-in limits, `null` for the config
/// file's value. They count for every way of signing in.
pub async fn set_limits(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(limits): Body<Limits>,
) -> Result<Json<WebView>, Error> {
    admin(&me)?;
    let within =
        |value: Option<u32>, low: u32, high: u32| value.is_none_or(|v| (low..=high).contains(&v));
    let applied = limits.apply(&state.config.auth);
    if !within(limits.per_minute, 1, 600)
        || !within(limits.attempts, 1, 100)
        || !within(limits.first_wait, 1, 1440)
        || !within(limits.longest_wait, 1, 10080)
        || applied.lockout_max_minutes < applied.lockout_minutes
    {
        return Err(Error::invalid("a sign-in limit is out of range"));
    }
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    state
        .db
        .call(move |c| {
            store::set_limits(c, &limits)?;
            admin_event(c, now, "signin_limits_changed", None, &me, &ip_text, "")
        })
        .await?;
    state.rate_limit.set(applied.rate_limit_per_minute);
    Ok(Json(view(&state).await?))
}

/// `POST /api/v1/web/session`: the cookie that opens the web version, for
/// the device that just signed in on its sign-in page.
pub async fn session(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
) -> Result<Response, Error> {
    let now = state.clock.now();
    let days = state.config.auth.refresh_token_days;
    let device = me.device_id.clone();
    let token = state
        .db
        .call(move |c| {
            if webapp::access(c)? != Access::SignedIn {
                return Ok(None);
            }
            Ok(Some(webapp::start_session(c, &device, now, days)?))
        })
        .await?
        .ok_or_else(Error::not_found)?;
    let mut response = StatusCode::NO_CONTENT.into_response();
    let cookie = HeaderValue::from_str(&webapp::cookie(&state, &token))
        .map_err(|_| Error::invalid("the server's address cannot go in a cookie"))?;
    response.headers_mut().insert(header::SET_COOKIE, cookie);
    Ok(response)
}
