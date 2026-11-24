// SPDX-License-Identifier: AGPL-3.0-or-later

//! The admin api: the instance at a glance, its switches and the security
//! log. Every change is logged with the acting admin.

use app_protocol::account::Role;
use app_protocol::admin::{AuditPage, Overview};
use app_protocol::error_code;
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use serde::Deserialize;

use super::extract::{Body, ClientIp, CurrentUser};
use crate::accounts::factors;
use crate::accounts::store::{self, Authed, Event};
use crate::admin as admin_store;
use crate::db::DbError;
use crate::error::Error;
use crate::routes::AppState;

pub(super) fn admin(me: &Authed) -> Result<(), Error> {
    if me.user.acts_as_admin() {
        Ok(())
    } else if me.user.role == Role::Admin {
        Err(Error::new(
            StatusCode::FORBIDDEN,
            error_code::SECOND_FACTOR_REQUIRED,
            "add a passkey or an authenticator app to your account first",
        ))
    } else {
        Err(Error::forbidden())
    }
}

/// Logs an admin action: `subject` is the account acted on, the acting
/// admin goes in the detail.
pub(super) fn admin_event(
    conn: &rusqlite::Connection,
    now: i64,
    name: &str,
    subject: Option<&str>,
    me: &Authed,
    ip: &str,
    more: &str,
) -> Result<(), DbError> {
    let detail = if more.is_empty() {
        format!("by {}", me.user.username)
    } else {
        format!("by {}: {more}", me.user.username)
    };
    store::audit(
        conn,
        now,
        &Event {
            name,
            user_id: subject,
            ip: Some(ip),
            detail: Some(&detail),
            ..Event::default()
        },
    )
}

/// `GET /api/v1/admin/overview`
pub async fn overview(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
) -> Result<Json<Overview>, Error> {
    admin(&me)?;
    Ok(Json(state.db.call(|c| admin_store::overview(c)).await?))
}

/// `PUT /api/v1/admin/coediting`
pub async fn set_coediting(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(req): Body<app_protocol::admin::CoeditingRequest>,
) -> Result<Json<app_protocol::admin::CoeditingRequest>, Error> {
    admin(&me)?;
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    let on = req.coediting;
    state
        .db
        .call(move |c| {
            crate::sync::set_coediting(c, on)?;
            let what = if on { "on" } else { "off" };
            admin_event(c, now, "coediting_changed", None, &me, &ip_text, what)
        })
        .await?;
    // devices connected now hear it at once
    state.live.settings(on);
    Ok(Json(req))
}

/// `PUT /api/v1/admin/second-step`: whether admins must have a second
/// step. Off, every admin acts as admin with a password alone.
pub async fn set_second_step(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(req): Body<app_protocol::admin::SecondStepRequest>,
) -> Result<Json<app_protocol::admin::SecondStepRequest>, Error> {
    admin(&me)?;
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    let required = req.required;
    state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            factors::set_admins_need_second_step(&tx, required)?;
            let what = if required { "required" } else { "optional" };
            admin_event(
                &tx,
                now,
                "admin_second_step_changed",
                None,
                &me,
                &ip_text,
                what,
            )?;
            tx.commit()?;
            Ok(())
        })
        .await?;
    Ok(Json(req))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditQuery {
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    before: Option<i64>,
    #[serde(default)]
    limit: Option<u32>,
}

/// `GET /api/v1/admin/audit?event=&userId=&before=&limit=`
pub async fn audit(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
    Query(q): Query<AuditQuery>,
) -> Result<Json<AuditPage>, Error> {
    admin(&me)?;
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let event = q.event.filter(|e| !e.is_empty());
    let user_id = q.user_id.filter(|u| !u.is_empty());
    let items = state
        .db
        .call(move |c| {
            admin_store::audit_page(c, event.as_deref(), user_id.as_deref(), q.before, limit)
        })
        .await?;
    let full = items.len() == usize::try_from(limit).unwrap_or(usize::MAX);
    let next_before = if full {
        items.last().map(|e| e.id)
    } else {
        None
    };
    Ok(Json(AuditPage { items, next_before }))
}

/// `POST /api/v1/admin/mail/test`: sends a test mail and waits for the mail
/// server, so the admin sees at once whether `[smtp]` works.
pub async fn mail_test(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
    Body(req): Body<app_protocol::admin::MailTestRequest>,
) -> Result<StatusCode, Error> {
    admin(&me)?;
    if !state.mailer.enabled() {
        return Err(Error::new(
            StatusCode::NOT_FOUND,
            error_code::MAIL_OFF,
            "email is not set up, see [smtp] in the configuration",
        ));
    }
    let to = crate::mail::plausible(&req.to).ok_or_else(|| {
        Error::new(
            StatusCode::BAD_REQUEST,
            error_code::EMAIL_INVALID,
            "this does not look like an email address",
        )
    })?;
    let host = url::Url::parse(&state.config.server.public_url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_default();
    let subject = crate::mail::tr("mail.test.subject", &[]);
    let body = crate::mail::tr(
        "mail.test.body",
        &[("app", app_brand::DISPLAY_NAME), ("host", &host)],
    );
    state
        .mailer
        .send_now(&to, subject, body)
        .await
        .map_err(|why| Error::new(StatusCode::BAD_GATEWAY, error_code::MAIL_FAILED, why))?;
    Ok(StatusCode::NO_CONTENT)
}
