// SPDX-License-Identifier: AGPL-3.0-or-later

//! The admin api: the instance at a glance, people, the registration mode
//! and the security log. Groups are in [`super::groups`]. Every change is
//! logged with the acting admin. Nothing here can leave the server without
//! an active admin.

use app_protocol::account::{CreateUserRequest, Device, RegistrationRequest, Role, User};
use app_protocol::admin::{
    AdminUser, AdminUserDetail, AuditPage, Overview, Page, SetPasswordRequest, UpdateUserRequest,
};
use app_protocol::e2ee::{AccountKind, AccountKindsRequest};
use app_protocol::error_code;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::Deserialize;

use super::auth::{hash_password, kind_allowed, kind_closed, new_credentials, taken};
use super::extract::{Body, ClientIp, CurrentUser, Language};
use super::user_json;
use crate::accounts::store::{self, Authed, Event};
use crate::accounts::{display_name, e2ee, factors, password};
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

fn no_user() -> Error {
    Error::new(
        StatusCode::NOT_FOUND,
        error_code::NOT_FOUND,
        "no such account",
    )
}

fn last_admin() -> Error {
    Error::new(
        StatusCode::CONFLICT,
        error_code::LAST_ADMIN,
        "the server needs at least one active admin",
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

/// `POST /api/v1/admin/users`
pub async fn create_user(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(req): Body<CreateUserRequest>,
) -> Result<(StatusCode, Json<User>), Error> {
    admin(&me)?;
    // the admin types the password, the server sees it: a standard account
    let kinds = state.db.call(|c| e2ee::account_kinds(c)).await?;
    if !kind_allowed(kinds, AccountKind::Standard) {
        return Err(kind_closed());
    }
    let username = new_credentials(&req.username, &req.password)?;
    let display = display_name(req.display_name.as_deref(), &username);
    let role = req.role.unwrap_or(Role::User);
    let hash = hash_password(&state, req.password).await?;
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    let user = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let Some(user) = store::insert_user(&tx, &username, &display, &hash, role, now)? else {
                return Ok(None);
            };
            // a new admin has no second factor yet: they add one first
            factors::guard_admin(&tx, &user.id)?;
            let user = store::user_by_id(&tx, &user.id)?.unwrap_or(user);
            admin_event(&tx, now, "user_created", Some(&user.id), &me, &ip_text, "")?;
            tx.commit()?;
            Ok(Some(user))
        })
        .await?
        .ok_or_else(taken)?;
    Ok((StatusCode::CREATED, Json(user_json(&user))))
}

/// Search and paging for the account list.
#[derive(Debug, Deserialize)]
pub struct ListQuery {
    /// Part of a username or shown name.
    #[serde(default)]
    query: String,
    #[serde(default)]
    offset: u32,
    #[serde(default)]
    limit: Option<u32>,
}

/// `GET /api/v1/admin/users?query=&offset=&limit=`
pub async fn users(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
    Query(q): Query<ListQuery>,
) -> Result<Json<Page<AdminUser>>, Error> {
    admin(&me)?;
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let (total, items) = state
        .db
        .call(move |c| admin_store::list_users(c, q.query.trim(), q.offset, limit))
        .await?;
    Ok(Json(Page { total, items }))
}

/// `GET /api/v1/admin/users/{id}`
pub async fn user(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
    Path(id): Path<String>,
) -> Result<Json<AdminUserDetail>, Error> {
    admin(&me)?;
    let found = state
        .db
        .call(move |c| {
            let Some(user) = admin_store::admin_user(c, &id)? else {
                return Ok(None);
            };
            let devices = store::list_devices(c, &id)?;
            Ok(Some((user, devices)))
        })
        .await?;
    let (user, devices) = found.ok_or_else(no_user)?;
    let device_list = devices
        .into_iter()
        .map(|d| Device {
            id: d.id,
            name: d.name,
            platform: d.platform,
            created_at: d.created_at,
            last_seen_at: d.last_seen_at,
            current: false,
        })
        .collect();
    Ok(Json(AdminUserDetail { user, device_list }))
}

/// What a change does, for the log.
fn changes(before: &AdminUser, req: &UpdateUserRequest, renamed: bool) -> String {
    let mut changes = Vec::new();
    if renamed {
        changes.push("name".to_string());
    }
    if let Some(role) = req.role.filter(|r| *r != before.role) {
        changes.push(format!(
            "role {}",
            if role == Role::Admin { "admin" } else { "user" }
        ));
    }
    match req.disabled {
        Some(true) if before.disabled_at.is_none() => changes.push("turned off".into()),
        Some(false) if before.disabled_at.is_some() => changes.push("turned on".into()),
        _ => {}
    }
    if req.unlock {
        changes.push("unlocked".into());
    }
    changes.join(", ")
}

/// `PATCH /api/v1/admin/users/{id}`
pub async fn update_user(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Path(id): Path<String>,
    Body(req): Body<UpdateUserRequest>,
) -> Result<Json<AdminUser>, Error> {
    admin(&me)?;
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    let display = req
        .display_name
        .as_deref()
        .map(|d| display_name(Some(d), ""));
    if display.as_deref() == Some("") {
        return Err(Error::invalid("the shown name cannot be empty"));
    }
    let after = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let Some(before) = admin_store::admin_user(&tx, &id)? else {
                return Ok(Err(no_user()));
            };
            let stays_active_admin = req.role.unwrap_or(before.role) == Role::Admin
                && !req.disabled.unwrap_or(before.disabled_at.is_some());
            if before.role == Role::Admin
                && !stays_active_admin
                && admin_store::other_active_admins(&tx, &id)? == 0
            {
                return Ok(Err(last_admin()));
            }
            admin_store::update_user(
                &tx,
                &id,
                display.as_deref(),
                req.role,
                req.disabled,
                req.unlock,
                now,
            )?;
            let what = changes(&before, &req, display.is_some());
            admin_event(&tx, now, "user_updated", Some(&id), &me, &ip_text, &what)?;
            let after = admin_store::admin_user(&tx, &id)?;
            tx.commit()?;
            Ok(after.ok_or_else(no_user))
        })
        .await??;
    Ok(Json(after))
}

/// `POST /api/v1/admin/users/{id}/password`: signs out every device.
pub async fn set_password(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Path(id): Path<String>,
    Body(req): Body<SetPasswordRequest>,
) -> Result<StatusCode, Error> {
    admin(&me)?;
    let lookup = id.clone();
    let user = state
        .db
        .call(move |c| store::user_by_id(c, &lookup))
        .await?
        .ok_or_else(no_user)?;
    if user.kind == AccountKind::E2ee {
        return Err(Error::new(
            StatusCode::CONFLICT,
            error_code::E2EE_ACCOUNT,
            "only the person sets the password of an end-to-end encrypted account",
        ));
    }
    if let Some(why) = password::weakness(&req.password, &user.username) {
        return Err(Error::new(
            StatusCode::BAD_REQUEST,
            error_code::PASSWORD_WEAK,
            why,
        ));
    }
    let hash = hash_password(&state, req.password).await?;
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            store::set_password(&tx, &id, &hash)?;
            store::revoke_other_devices(&tx, &id, None, now)?;
            store::record_success(&tx, &id)?;
            admin_event(&tx, now, "password_reset", Some(&id), &me, &ip_text, "")?;
            tx.commit()?;
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/v1/admin/users/{id}/signout`: every device of the account.
pub async fn sign_out(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Path(id): Path<String>,
) -> Result<StatusCode, Error> {
    admin(&me)?;
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    let found = state
        .db
        .call(move |c| {
            if store::user_by_id(c, &id)?.is_none() {
                return Ok(false);
            }
            let n = store::revoke_other_devices(c, &id, None, now)?;
            let what = format!("{n} devices");
            admin_event(
                c,
                now,
                "signed_out_everywhere",
                Some(&id),
                &me,
                &ip_text,
                &what,
            )?;
            Ok(true)
        })
        .await?;
    if found {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(no_user())
    }
}

/// `POST /api/v1/admin/users/{id}/second-factors/reset`: for someone who
/// lost their phone and their recovery codes. The password alone signs in
/// again, every device is signed out.
pub async fn reset_second_factors(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Path(id): Path<String>,
) -> Result<StatusCode, Error> {
    admin(&me)?;
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    let found = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            if store::user_by_id(&tx, &id)?.is_none() {
                return Ok(false);
            }
            factors::reset_second_factors(&tx, &id)?;
            store::revoke_other_devices(&tx, &id, None, now)?;
            admin_event(
                &tx,
                now,
                "second_factors_reset",
                Some(&id),
                &me,
                &ip_text,
                "devices signed out",
            )?;
            tx.commit()?;
            Ok(true)
        })
        .await?;
    if found {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(no_user())
    }
}

/// `DELETE /api/v1/admin/users/{id}`
pub async fn delete_user(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Path(id): Path<String>,
) -> Result<StatusCode, Error> {
    admin(&me)?;
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let Some(user) = store::user_by_id(&tx, &id)? else {
                return Ok(Err(no_user()));
            };
            if user.role == Role::Admin && admin_store::other_active_admins(&tx, &id)? == 0 {
                return Ok(Err(last_admin()));
            }
            admin_store::delete_user(&tx, &id)?;
            // the account is gone, the log keeps its name
            admin_event(
                &tx,
                now,
                "user_deleted",
                None,
                &me,
                &ip_text,
                &user.username,
            )?;
            tx.commit()?;
            Ok(Ok(()))
        })
        .await??;
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /api/v1/admin/registration`
pub async fn registration(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
) -> Result<Json<RegistrationRequest>, Error> {
    admin(&me)?;
    let registration = state.db.call(|c| store::registration(c)).await?;
    Ok(Json(RegistrationRequest { registration }))
}

/// `PUT /api/v1/admin/registration`
pub async fn set_registration(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(req): Body<RegistrationRequest>,
) -> Result<Json<RegistrationRequest>, Error> {
    admin(&me)?;
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    let mode = req.registration;
    state
        .db
        .call(move |c| {
            store::set_registration(c, mode)?;
            let what = format!("{mode:?}").to_lowercase();
            admin_event(c, now, "registration_changed", None, &me, &ip_text, &what)
        })
        .await?;
    Ok(Json(RegistrationRequest { registration: mode }))
}

/// `PUT /api/v1/admin/account-kinds`: which kinds sign-up creates.
/// Existing accounts keep their kind.
pub async fn set_account_kinds(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(req): Body<AccountKindsRequest>,
) -> Result<Json<AccountKindsRequest>, Error> {
    admin(&me)?;
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    let kinds = req.account_kinds;
    state
        .db
        .call(move |c| {
            e2ee::set_account_kinds(c, kinds)?;
            let what = super::e2ee::kinds_logged(kinds);
            admin_event(c, now, "account_kinds_changed", None, &me, &ip_text, what)
        })
        .await?;
    Ok(Json(AccountKindsRequest {
        account_kinds: kinds,
    }))
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
    if !on {
        // presence goes with co-editing: nobody is shown in a note any more
        let spaces = state.live.clear_seats();
        super::sync::announce(&state, spaces).await;
    }
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

/// `PUT /api/v1/admin/personal`: the server for its admin alone, or open
/// to others. Only what the admin page shows changes; sign-up stays as it
/// is until changed there.
pub async fn set_personal(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(req): Body<app_protocol::admin::PersonalRequest>,
) -> Result<Json<app_protocol::admin::PersonalRequest>, Error> {
    admin(&me)?;
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    let personal = req.personal;
    state
        .db
        .call(move |c| {
            crate::admin::set_personal(c, personal)?;
            let what = if personal { "personal" } else { "instance" };
            admin_event(c, now, "server_use_changed", None, &me, &ip_text, what)
        })
        .await?;
    Ok(Json(req))
}

/// Filters for the security log.
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
    Language(lang): Language,
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
    let subject = crate::mail::tr(lang, "mail.test.subject", &[]);
    let body = crate::mail::tr(
        lang,
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
