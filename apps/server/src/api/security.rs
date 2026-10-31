// SPDX-License-Identifier: AGPL-3.0-or-later

//! How the signed in account signs in: the authenticator app and recovery
//! codes. Turning a factor on or off, or new recovery codes, need the
//! password again, so a borrowed unlocked device cannot change them.

use app_protocol::e2ee::AccountKind;
use app_protocol::error_code;
use app_protocol::security::{Confirm, RecoveryCodes, Security, TotpConfirmRequest, TotpSetup};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;

use super::auth::check_password;
use super::extract::{Body, ClientIp, CurrentUser};
use crate::accounts::factors;
use crate::accounts::password::Check;
use crate::accounts::store::{self, Authed, Event};
use crate::accounts::totp;
use crate::clock::MINUTE;
use crate::error::Error;
use crate::routes::AppState;

/// An account without a password proves itself by a sign-in this recent.
const FRESH_SIGN_IN: i64 = 10 * MINUTE;

/// Checks the person is the owner: the password, the proof of an OPAQUE
/// run for an e2ee account, or for an account without a password, a
/// device signed in moments ago.
pub(super) async fn confirm(
    state: &AppState,
    me: &Authed,
    given: Confirm,
    ip: ClientIp,
) -> Result<(), Error> {
    let now = state.clock.now();
    if me.user.kind == AccountKind::E2ee {
        let proof = given.proof.unwrap_or_default();
        return if !proof.is_empty() && super::e2ee::spend_proof(state, &me.user.id, &proof).await? {
            Ok(())
        } else {
            Err(confirm_needed())
        };
    }
    let password = given.password;
    if me.user.password_hash.is_empty() {
        let device = me.device_id.clone();
        let since = state
            .db
            .call(move |c| factors::device_signed_in_at(c, &device))
            .await?;
        return if since.is_some_and(|t| now - t <= FRESH_SIGN_IN) {
            Ok(())
        } else {
            Err(confirm_needed())
        };
    }
    let Some(password) = password.filter(|p| !p.is_empty()) else {
        return Err(confirm_needed());
    };
    // as a sign-in: a stolen session guesses no faster than anyone
    super::auth::rate_limit(state, ip)?;
    super::auth::not_locked(&me.user, now)?;
    if check_password(state, password, Some(me.user.password_hash.clone())).await? != Check::Bad {
        return Ok(());
    }
    let id = me.user.id.clone();
    let auth = state.config.auth.clone();
    let ip = ip.0.to_string();
    state
        .db
        .call(move |c| {
            store::record_failure(c, &id, now, &auth)?;
            store::audit(
                c,
                now,
                &Event {
                    name: "confirm_failed",
                    user_id: Some(&id),
                    ip: Some(&ip),
                    ..Event::default()
                },
            )
        })
        .await?;
    Err(Error::new(
        StatusCode::UNAUTHORIZED,
        error_code::BAD_CREDENTIALS,
        "the password is wrong",
    ))
}

fn confirm_needed() -> Error {
    Error::new(
        StatusCode::FORBIDDEN,
        error_code::CONFIRM_NEEDED,
        "enter your password, or sign in again, to change this",
    )
}

/// Logs a change to how the account signs in.
pub(super) async fn log(
    state: &AppState,
    me: &Authed,
    ip: ClientIp,
    name: &'static str,
) -> Result<(), Error> {
    let now = state.clock.now();
    let id = me.user.id.clone();
    let device = me.device_id.clone();
    let ip = ip.0.to_string();
    state
        .db
        .call(move |c| {
            store::audit(
                c,
                now,
                &Event {
                    name,
                    user_id: Some(&id),
                    device_id: Some(&device),
                    ip: Some(&ip),
                    ..Event::default()
                },
            )
        })
        .await?;
    Ok(())
}

/// `GET /api/v1/me/security`
pub async fn security(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
) -> Result<Json<Security>, Error> {
    let id = me.user.id.clone();
    let (totp, codes, email, delete_at, email_codes) = state
        .db
        .call(move |c| {
            Ok((
                factors::totp(c, &id)?.secret.is_some(),
                factors::recovery_codes_left(c, &id)?,
                crate::accounts::profile::email(c, &id)?,
                crate::accounts::profile::delete_at(c, &id)?,
                factors::email_codes(c, &id)?.is_some(),
            ))
        })
        .await?;
    Ok(Json(Security {
        has_password: me.user.kind == AccountKind::E2ee || !me.user.password_hash.is_empty(),
        totp,
        recovery_codes_left: codes,
        passkeys: super::passkeys::list(&state, &me.user.id).await?,
        sso: super::sso::list(&state, &me.user.id).await?,
        email,
        email_available: state.mailer.enabled(),
        email_codes,
        delete_at,
    }))
}

/// `POST /api/v1/me/totp/setup`: a new secret to scan. Nothing changes
/// until a code from it is confirmed.
pub async fn totp_setup(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
) -> Result<Json<TotpSetup>, Error> {
    let secret = totp::new_secret();
    let host = url::Url::parse(&state.config.server.public_url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned));
    let account = match host {
        Some(host) => format!("{}@{host}", me.user.username),
        None => me.user.username.clone(),
    };
    let uri = totp::uri(&secret, app_brand::DISPLAY_NAME, &account);
    let id = me.user.id.clone();
    let pending = secret.clone();
    let master = state.master.clone();
    state
        .db
        .call(move |c| factors::set_totp_pending(c, &master, &id, &pending))
        .await?;
    Ok(Json(TotpSetup {
        qr_svg: totp::qr_svg(&uri),
        secret,
        uri,
    }))
}

/// `POST /api/v1/me/totp/confirm`: turns the app on with a first code.
/// The first factor also brings recovery codes, shown in the answer.
pub async fn totp_confirm(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(req): Body<TotpConfirmRequest>,
) -> Result<Json<RecoveryCodes>, Error> {
    let given = Confirm {
        password: req.password,
        proof: req.proof,
    };
    confirm(&state, &me, given, ip).await?;
    let now = state.clock.now();
    let id = me.user.id.clone();
    let master = state.master.clone();
    let codes = totp::new_recovery_codes();
    let hashes: Vec<_> = codes.iter().map(|(_, h)| *h).collect();
    let issued = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let current = factors::totp(&tx, &id)?;
            let Some(step) = current
                .pending
                .as_deref()
                .and_then(|s| factors::open_totp(&master, &id, s))
                .and_then(|s| totp::check(&s, &req.code, now, None))
            else {
                return Ok(None);
            };
            factors::activate_totp(&tx, &id, step)?;
            store::set_factor_required(&tx, &id, false)?;
            let fresh = factors::recovery_codes_left(&tx, &id)? == 0;
            if fresh {
                factors::replace_recovery_codes(&tx, &id, &hashes)?;
            }
            tx.commit()?;
            Ok(Some(fresh))
        })
        .await?;
    let Some(fresh) = issued else {
        return Err(Error::new(
            StatusCode::BAD_REQUEST,
            error_code::TOTP_INVALID,
            "this code does not match, check the time on the phone",
        ));
    };
    log(&state, &me, ip, "totp_enabled").await?;
    let codes = if fresh {
        codes.into_iter().map(|(code, _)| code).collect()
    } else {
        Vec::new()
    };
    Ok(Json(RecoveryCodes { codes }))
}

/// `POST /api/v1/me/totp/disable`
pub async fn totp_disable(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(req): Body<Confirm>,
) -> Result<StatusCode, Error> {
    confirm(&state, &me, req, ip).await?;
    let id = me.user.id.clone();
    state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            factors::disable_totp(&tx, &id)?;
            factors::drop_unneeded_recovery_codes(&tx, &id)?;
            tx.commit()?;
            Ok(())
        })
        .await?;
    log(&state, &me, ip, "totp_disabled").await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/v1/me/email-codes/enable`: codes by email as the account's
/// second step. Needs a server that sends mail and a confirmed address.
pub async fn email_codes_enable(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
) -> Result<StatusCode, Error> {
    if !state.mailer.enabled() {
        return Err(Error::new(
            StatusCode::NOT_FOUND,
            error_code::MAIL_OFF,
            "this server sends no email",
        ));
    }
    let id = me.user.id.clone();
    let done = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            if crate::accounts::profile::email(&tx, &id)?.is_none() {
                return Ok(false);
            }
            factors::set_email_codes(&tx, &id, true)?;
            tx.commit()?;
            Ok(true)
        })
        .await?;
    if !done {
        return Err(Error::invalid("confirm an email address first"));
    }
    log(&state, &me, ip, "email_codes_enabled").await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/v1/me/email-codes/disable`, confirmed like any second step
/// going away.
pub async fn email_codes_disable(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(req): Body<Confirm>,
) -> Result<StatusCode, Error> {
    confirm(&state, &me, req, ip).await?;
    let id = me.user.id.clone();
    state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            factors::set_email_codes(&tx, &id, false)?;
            tx.commit()?;
            Ok(())
        })
        .await?;
    log(&state, &me, ip, "email_codes_disabled").await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/v1/me/recovery-codes`: a new set, the old one stops working.
pub async fn new_recovery_codes(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(req): Body<Confirm>,
) -> Result<Json<RecoveryCodes>, Error> {
    confirm(&state, &me, req, ip).await?;
    let id = me.user.id.clone();
    let codes = totp::new_recovery_codes();
    let hashes: Vec<_> = codes.iter().map(|(_, h)| *h).collect();
    let allowed = state
        .db
        .call(move |c| {
            if !factors::has_second_factor(c, &id)? {
                return Ok(false);
            }
            factors::replace_recovery_codes(c, &id, &hashes)?;
            Ok(true)
        })
        .await?;
    if !allowed {
        return Err(Error::new(
            StatusCode::CONFLICT,
            error_code::INVALID,
            "recovery codes come with a second factor, set one up first",
        ));
    }
    log(&state, &me, ip, "recovery_codes_created").await?;
    Ok(Json(RecoveryCodes {
        codes: codes.into_iter().map(|(code, _)| code).collect(),
    }))
}
