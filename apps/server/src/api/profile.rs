// SPDX-License-Identifier: AGPL-3.0-or-later

//! What people manage themselves: a confirmed email, a password reset by
//! email, the export of their data, deleting their account. And the facts
//! the privacy page shows.

use app_protocol::account::Role;
use app_protocol::e2ee::AccountKind;
use app_protocol::error_code;
use app_protocol::profile::{
    Deletion, DeletionRequest, EmailRequest, EmailVerified, EmailVerifyRequest, PrivacyInfo,
    ResetFinish, ResetRequest,
};
use app_protocol::security::Confirm;
use axum::Json;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use super::auth::{hash_password, rate_limit};
use super::extract::{Body, ClientIp, CurrentUser, Language};
use super::security::{confirm, log};
use crate::accounts::factors::{self, ChallengeKind};
use crate::accounts::store::{self, Event, UserRow};
use crate::accounts::{e2ee, password, profile, token};
use crate::admin as admin_store;
use crate::clock::{DAY, MINUTE};
use crate::error::Error;
use crate::mail::{Lang, plausible, tr};
use crate::routes::AppState;

/// How long an address link works.
const VERIFY_LIFE: i64 = DAY;
/// How long a reset link works.
const RESET_LIFE: i64 = 60 * MINUTE;

fn mail_off() -> Error {
    Error::new(
        StatusCode::NOT_FOUND,
        error_code::MAIL_OFF,
        "this server sends no email",
    )
}

fn link_invalid() -> Error {
    Error::new(
        StatusCode::BAD_REQUEST,
        error_code::LINK_INVALID,
        "this link is used, expired, or was never sent",
    )
}

fn host(state: &AppState) -> String {
    url::Url::parse(&state.config.server.public_url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_default()
}

fn link(state: &AppState, path: &str) -> String {
    format!(
        "{}{path}",
        state.config.server.public_url.trim_end_matches('/')
    )
}

/// Wraps a mail's text in the greeting and the footer, and sends it.
fn send(state: &AppState, lang: Lang, to: &str, user: &UserRow, subject: String, text: &str) {
    let greeting = tr(lang, "mail.greeting", &[("name", &user.display_name)]);
    let footer = tr(
        lang,
        "mail.footer",
        &[("app", app_brand::DISPLAY_NAME), ("host", &host(state))],
    );
    let body = format!("{greeting}\n\n{text}\n\n-- \n{footer}\n");
    state.mailer.send(to, subject, body);
}

/// A day as people read it in a mail, in UTC: the exact hour is on the
/// account page.
fn day(ms: i64) -> String {
    let days = ms.div_euclid(DAY);
    // civil date from days since 1970-01-01 (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// What an address link carries until it is opened.
#[derive(Debug, Serialize, Deserialize)]
struct PendingEmail {
    email: String,
}

/// `PUT /api/v1/me/email`: sends a link to the address. Nothing changes
/// until it is opened, so nobody keeps an address they cannot read.
pub async fn set_email(
    State(state): State<AppState>,
    ip: ClientIp,
    Language(lang): Language,
    CurrentUser(me): CurrentUser,
    Body(req): Body<EmailRequest>,
) -> Result<StatusCode, Error> {
    if !state.mailer.enabled() {
        return Err(mail_off());
    }
    let email = plausible(&req.email).ok_or_else(|| {
        Error::new(
            StatusCode::BAD_REQUEST,
            error_code::EMAIL_INVALID,
            "this is not an email address",
        )
    })?;
    let given = Confirm {
        password: req.password,
        proof: req.proof,
    };
    confirm(&state, &me, given, ip).await?;
    let now = state.clock.now();
    let id = me.user.id.clone();
    let pending = PendingEmail {
        email: email.clone(),
    };
    let token = state
        .db
        .call(move |c| {
            factors::create_challenge(
                c,
                ChallengeKind::EmailVerify,
                Some(&id),
                &pending,
                now + VERIFY_LIFE,
            )
        })
        .await?;
    let subject = tr(lang, "mail.verify.subject", &[]);
    let text = tr(
        lang,
        "mail.verify.body",
        &[
            ("username", &me.user.username),
            ("link", &link(&state, &format!("/verify/{token}"))),
        ],
    );
    send(&state, lang, &email, &me.user, subject, &text);
    log(&state, &me, ip, "email_link_sent").await?;
    Ok(StatusCode::ACCEPTED)
}

/// `DELETE /api/v1/me/email`
pub async fn remove_email(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(req): Body<Confirm>,
) -> Result<StatusCode, Error> {
    confirm(&state, &me, req, ip).await?;
    let now = state.clock.now();
    let id = me.user.id.clone();
    state
        .db
        .call(move |c| profile::set_email(c, &id, None, now))
        .await?;
    log(&state, &me, ip, "email_removed").await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/v1/auth/email/verify`: opening the link proves the address,
/// from any browser, signed in or not.
pub async fn verify_email(
    State(state): State<AppState>,
    ip: ClientIp,
    Body(req): Body<EmailVerifyRequest>,
) -> Result<Json<EmailVerified>, Error> {
    rate_limit(&state, ip)?;
    let now = state.clock.now();
    let hash = token::hash(req.token.trim());
    let ip_text = ip.0.to_string();
    let verified = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let found =
                factors::challenge::<PendingEmail>(&tx, &hash, ChallengeKind::EmailVerify, now)?;
            if !factors::end_challenge(&tx, &hash)? {
                return Ok(None);
            }
            let Some(found) = found else {
                return Ok(None);
            };
            let Some(user) = found
                .user_id
                .as_deref()
                .map(|id| store::user_by_id(&tx, id))
                .transpose()?
                .flatten()
            else {
                return Ok(None);
            };
            profile::set_email(&tx, &user.id, Some(&found.data.email), now)?;
            store::audit(
                &tx,
                now,
                &Event {
                    name: "email_confirmed",
                    user_id: Some(&user.id),
                    ip: Some(&ip_text),
                    ..Event::default()
                },
            )?;
            tx.commit()?;
            Ok(Some(EmailVerified {
                username: user.username,
                email: found.data.email,
            }))
        })
        .await?;
    verified.map(Json).ok_or_else(link_invalid)
}

/// `POST /api/v1/auth/reset/request`. The same 202 whatever happened. A
/// standard account with a confirmed address gets a link, an e2ee one a
/// pointer to its recovery code, nobody else anything.
pub async fn reset_request(
    State(state): State<AppState>,
    ip: ClientIp,
    Language(lang): Language,
    Body(req): Body<ResetRequest>,
) -> Result<StatusCode, Error> {
    rate_limit(&state, ip)?;
    if !state.mailer.enabled() {
        return Err(mail_off());
    }
    let now = state.clock.now();
    let name = req.username.trim().to_ascii_lowercase();
    let found = state
        .db
        .call(move |c| {
            let Some(user) = store::user_by_name(c, &name)? else {
                return Ok(None);
            };
            let Some(email) = profile::email(c, &user.id)? else {
                return Ok(None);
            };
            if user.disabled_at.is_some() {
                return Ok(None);
            }
            let token = if user.kind == AccountKind::Standard {
                Some(factors::create_challenge(
                    c,
                    ChallengeKind::PasswordReset,
                    Some(&user.id),
                    &(),
                    now + RESET_LIFE,
                )?)
            } else {
                None
            };
            Ok(Some((user, email, token)))
        })
        .await?;
    if let Some((user, email, token)) = found {
        let username = user.username.clone();
        let (subject, text) = match token {
            Some(token) => (
                tr(lang, "mail.reset.subject", &[]),
                tr(
                    lang,
                    "mail.reset.body",
                    &[
                        ("username", &username),
                        ("link", &link(&state, &format!("/reset/{token}"))),
                    ],
                ),
            ),
            None => (
                tr(lang, "mail.recover.subject", &[]),
                tr(
                    lang,
                    "mail.recover.body",
                    &[("username", &username), ("link", &link(&state, "/recover"))],
                ),
            ),
        };
        send(&state, lang, &email, &user, subject, &text);
    }
    Ok(StatusCode::ACCEPTED)
}

/// `POST /api/v1/auth/reset/finish`: the new password. The link proves the
/// mailbox, not the person: the second factor still applies at the next
/// sign-in, and the address hears about the change.
pub async fn reset_finish(
    State(state): State<AppState>,
    ip: ClientIp,
    Language(lang): Language,
    Body(req): Body<ResetFinish>,
) -> Result<StatusCode, Error> {
    rate_limit(&state, ip)?;
    let now = state.clock.now();
    let hash = token::hash(req.token.trim());
    let user = state
        .db
        .call(move |c| {
            let found = factors::challenge::<()>(c, &hash, ChallengeKind::PasswordReset, now)?;
            Ok(match found.and_then(|f| f.user_id) {
                Some(id) => store::user_by_id(c, &id)?,
                None => None,
            })
        })
        .await?
        // a turned off account takes no new password by email either
        .filter(|u| u.kind == AccountKind::Standard && u.disabled_at.is_none())
        .ok_or_else(link_invalid)?;
    if let Some(why) = password::weakness(&req.password, &user.username) {
        return Err(Error::new(
            StatusCode::BAD_REQUEST,
            error_code::PASSWORD_WEAK,
            why,
        ));
    }
    let new_hash = hash_password(&state, req.password).await?;
    let hash = token::hash(req.token.trim());
    let id = user.id.clone();
    let ip_text = ip.0.to_string();
    let email = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            // spent here, in the same transaction as the change
            if !factors::end_challenge(&tx, &hash)? {
                return Ok(None);
            }
            store::set_password(&tx, &id, &new_hash)?;
            let n = store::revoke_other_devices(&tx, &id, None, now)?;
            store::record_success(&tx, &id)?;
            let detail = format!("by email, {n} devices signed out");
            store::audit(
                &tx,
                now,
                &Event {
                    name: "password_reset_by_email",
                    user_id: Some(&id),
                    ip: Some(&ip_text),
                    detail: Some(&detail),
                    ..Event::default()
                },
            )?;
            let email = profile::email(&tx, &id)?;
            tx.commit()?;
            Ok(Some(email))
        })
        .await?
        .ok_or_else(link_invalid)?;
    if let Some(email) = email {
        let subject = tr(lang, "mail.password.subject", &[]);
        let text = tr(lang, "mail.password.body", &[("username", &user.username)]);
        send(&state, lang, &email, &user, subject, &text);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /api/v1/me/export`: everything the server keeps for the account,
/// as a json file.
pub async fn export(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
) -> Result<Response, Error> {
    let now = state.clock.now();
    let id = me.user.id.clone();
    let data = state.db.call(move |c| profile::export(c, &id, now)).await?;
    log(&state, &me, ip, "data_exported").await?;
    let body = serde_json::to_vec_pretty(&data).map_err(|e| Error::internal(&e.to_string()))?;
    let mut response = (StatusCode::OK, body).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    let name = format!(
        "attachment; filename=\"{}-{}.json\"",
        app_brand::SLUG,
        me.user.username
    );
    if let Ok(value) = HeaderValue::from_str(&name) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    Ok(response)
}

fn last_admin() -> Error {
    Error::new(
        StatusCode::CONFLICT,
        error_code::LAST_ADMIN,
        "the server needs at least one active admin",
    )
}

/// `POST /api/v1/me/deletion`: the account goes after the grace period,
/// other devices are signed out now. Signing in and cancelling before the
/// date keeps it. Without a grace period it goes at once.
pub async fn request_deletion(
    State(state): State<AppState>,
    ip: ClientIp,
    Language(lang): Language,
    CurrentUser(me): CurrentUser,
    Body(req): Body<DeletionRequest>,
) -> Result<Response, Error> {
    let given = Confirm {
        password: req.password,
        proof: req.proof,
    };
    confirm(&state, &me, given, ip).await?;
    let now = state.clock.now();
    let grace = i64::from(state.config.auth.deletion_grace_days) * DAY;
    let delete_at = now + grace;
    let id = me.user.id.clone();
    let is_admin = me.user.role == Role::Admin;
    let device = me.device_id.clone();
    let ip_text = ip.0.to_string();
    let username = me.user.username.clone();
    let outcome = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            if is_admin && admin_store::other_active_admins(&tx, &id)? == 0 {
                return Ok(Err(last_admin()));
            }
            let email = profile::email(&tx, &id)?;
            if grace == 0 {
                admin_store::delete_user(&tx, &id)?;
                store::audit(
                    &tx,
                    now,
                    &Event {
                        name: "user_deleted",
                        ip: Some(&ip_text),
                        detail: Some(&format!("{username}, by themselves")),
                        ..Event::default()
                    },
                )?;
                tx.commit()?;
                return Ok(Ok(None));
            }
            profile::set_delete_at(&tx, &id, Some(delete_at))?;
            store::revoke_other_devices(&tx, &id, Some(&device), now)?;
            store::audit(
                &tx,
                now,
                &Event {
                    name: "deletion_requested",
                    user_id: Some(&id),
                    device_id: Some(&device),
                    ip: Some(&ip_text),
                    detail: Some(&format!("on {}", day(delete_at))),
                },
            )?;
            tx.commit()?;
            Ok(Ok(Some(email)))
        })
        .await??;
    let Some(email) = outcome else {
        return Ok(StatusCode::NO_CONTENT.into_response());
    };
    if let Some(email) = email {
        let subject = tr(lang, "mail.deletion.subject", &[]);
        let text = tr(
            lang,
            "mail.deletion.body",
            &[("username", &me.user.username), ("date", &day(delete_at))],
        );
        send(&state, lang, &email, &me.user, subject, &text);
    }
    Ok((StatusCode::ACCEPTED, Json(Deletion { delete_at })).into_response())
}

/// `DELETE /api/v1/me/deletion`: keeps the account.
pub async fn cancel_deletion(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
) -> Result<StatusCode, Error> {
    let id = me.user.id.clone();
    state
        .db
        .call(move |c| profile::set_delete_at(c, &id, None))
        .await?;
    log(&state, &me, ip, "deletion_cancelled").await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Deletes the accounts whose grace period is over. The last active admin
/// stays, an admin has to hand over first: logged, tried again next time.
pub async fn run_deletions(state: &AppState) -> Result<usize, Error> {
    let now = state.clock.now();
    let deleted = state
        .db
        .call(move |c| {
            let mut deleted = 0;
            for (id, username) in profile::due_deletions(c, now)? {
                let tx = c.transaction()?;
                let user = store::user_by_id(&tx, &id)?;
                if user.is_some_and(|u| u.role == Role::Admin)
                    && admin_store::other_active_admins(&tx, &id)? == 0
                {
                    tracing::warn!("{username} asked to be deleted but is the last admin, kept");
                    continue;
                }
                admin_store::delete_user(&tx, &id)?;
                store::audit(
                    &tx,
                    now,
                    &Event {
                        name: "user_deleted",
                        detail: Some(&format!("{username}, after the grace period")),
                        ..Event::default()
                    },
                )?;
                tx.commit()?;
                deleted += 1;
            }
            Ok(deleted)
        })
        .await?;
    Ok(deleted)
}

/// `GET /api/v1/privacy`
pub async fn privacy(State(state): State<AppState>) -> Result<Json<PrivacyInfo>, Error> {
    let kinds = state.db.call(|c| e2ee::account_kinds(c)).await?;
    let config = &state.config;
    let feedback_config = config.feedback.clone();
    let feedback = state
        .db
        .call(move |c| crate::feedback::settings(c, &feedback_config))
        .await?
        .info();
    let main_host = app_brand::FEEDBACK_URL
        .split("://")
        .nth(1)
        .unwrap_or_default()
        .trim_end_matches('/')
        .to_owned();
    let given = |s: &str| Some(s.trim().to_owned()).filter(|s| !s.is_empty());
    Ok(Json(PrivacyInfo {
        operator: given(&config.privacy.operator),
        contact: given(&config.privacy.contact),
        notice: given(&config.privacy.notice),
        account_kinds: kinds,
        email: state.mailer.enabled(),
        sso: state.sso.as_ref().map(|_| config.sso.label.clone()),
        audit_days: config.auth.audit_days,
        deletion_grace_days: config.auth.deletion_grace_days,
        session_days: config.auth.refresh_token_days,
        feedback_receive: feedback.receive,
        feedback_keep_days: config.feedback.keep_days,
        feedback_send_to: feedback.send_to,
        feedback_main: main_host,
    }))
}

#[cfg(test)]
mod tests {
    use super::day;

    #[test]
    fn days_in_mails() {
        assert_eq!(day(0), "1970-01-01");
        assert_eq!(day(951_782_400_000), "2000-02-29");
        assert_eq!(day(1_792_972_800_000), "2026-10-26");
    }
}
