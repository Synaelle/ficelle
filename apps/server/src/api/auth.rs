// SPDX-License-Identifier: AGPL-3.0-or-later

//! Signing in and out: sign-in, refresh.
//! Each of these is rate limited per address.

use app_protocol::account::{
    AuthOptions, DeviceInfo, LoginRequest, RefreshRequest, Registration, Session,
};
use app_protocol::e2ee::AccountKinds;
use app_protocol::error_code;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use super::extract::{Body, ClientIp, CurrentUser};
use super::second_factor;
use super::user_json;
use crate::accounts::password::{self, Check};
use crate::accounts::store::{self, Event, RefreshRefused, Tokens, UserRow};
use crate::accounts::{device_info, normal_username, token};
use crate::error::Error;
use crate::routes::AppState;

pub(super) fn rate_limit(state: &AppState, ip: ClientIp) -> Result<(), Error> {
    state
        .rate_limit
        .check(ip.0, state.clock.now())
        .map_err(|wait| {
            Error::new(
                StatusCode::TOO_MANY_REQUESTS,
                error_code::RATE_LIMITED,
                "too many attempts, wait a moment",
            )
            .retry_after(wait)
        })
}

pub(super) fn session(user: &UserRow, tokens: Tokens) -> Session {
    Session {
        access_token: tokens.access,
        access_expires_at: tokens.access_expires_at,
        refresh_token: tokens.refresh,
        refresh_expires_at: tokens.refresh_expires_at,
        device_id: tokens.device_id,
        user: user_json(user),
    }
}

/// Hashing takes tens of milliseconds of CPU, off the async workers.
pub(super) async fn hash_password(state: &AppState, password: String) -> Result<String, Error> {
    let passwords = state.passwords.clone();
    let _turn = hashing_turn(state).await?;
    tokio::task::spawn_blocking(move || passwords.hash(&password))
        .await
        .map_err(|e| Error::internal(&e.to_string()))?
        .map_err(|e| Error::internal(&e))
}

pub(super) async fn check_password(
    state: &AppState,
    password: String,
    stored: Option<String>,
) -> Result<Check, Error> {
    // longer than any password may be: no hashing for it
    if password.len() > password::MAX_BYTES {
        return Ok(Check::Bad);
    }
    let passwords = state.passwords.clone();
    let _turn = hashing_turn(state).await?;
    tokio::task::spawn_blocking(move || passwords.check(&password, stored.as_deref()))
        .await
        .map_err(|e| Error::internal(&e.to_string()))
}

/// A few password hashes at a time, each takes its own memory: many at once
/// would run the server out of it.
async fn hashing_turn(state: &AppState) -> Result<tokio::sync::SemaphorePermit<'_>, Error> {
    state
        .hashing
        .acquire()
        .await
        .map_err(|e| Error::internal(&e.to_string()))
}

/// A username and password fit for a new account.
pub(super) fn new_credentials(username: &str, password: &str) -> Result<String, Error> {
    let username = normal_username(username).ok_or_else(|| {
        Error::new(
            StatusCode::BAD_REQUEST,
            error_code::USERNAME_INVALID,
            "use 3 to 32 of a-z, 0-9, dot, dash and underscore, starting with a letter or digit",
        )
    })?;
    if let Some(why) = password::weakness(password, &username) {
        return Err(Error::new(
            StatusCode::BAD_REQUEST,
            error_code::PASSWORD_WEAK,
            why,
        ));
    }
    Ok(username)
}

/// `GET /api/v1/auth/options`
pub async fn options(State(state): State<AppState>) -> Result<Json<AuthOptions>, Error> {
    let (locked, account_kinds) = state
        .db
        .call(|c| Ok((crate::setup::is_locked(c)?, kinds_here(c)?)))
        .await?;
    Ok(Json(AuthOptions {
        registration: Registration::Closed,
        setup_needed: !locked,
        account_kinds,
        email_reset: state.mailer.enabled(),
        passkeys: state.webauthn.is_some(),
        sso: None,
    }))
}

/// The kind every account here has, the choice when they differ or there
/// are none: the sign-in pages offer a passkey to an app only where no
/// account is encrypted, whose notes open with the password alone.
fn kinds_here(conn: &rusqlite::Connection) -> Result<AccountKinds, crate::db::DbError> {
    let (standard, e2ee): (i64, i64) = conn.query_row(
        "SELECT COUNT(*) FILTER (WHERE kind = 'standard'), COUNT(*) FILTER (WHERE kind = 'e2ee') FROM users",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(match (standard, e2ee) {
        (s, 0) if s > 0 => AccountKinds::Standard,
        (0, e) if e > 0 => AccountKinds::E2ee,
        _ => AccountKinds::Choice,
    })
}

/// `POST /api/v1/auth/login`: a session, or 202 and a challenge when the
/// account has a second factor.
pub async fn login(
    State(state): State<AppState>,
    ip: ClientIp,
    Body(req): Body<LoginRequest>,
) -> Result<Response, Error> {
    rate_limit(&state, ip)?;
    let now = state.clock.now();
    let name = req.username.trim().to_ascii_lowercase();
    let lookup = name.clone();
    let user = state
        .db
        .call(move |c| store::user_by_name(c, &lookup))
        .await?;
    not_locked(&state, user.as_ref(), &name, ip, now).await?;
    // an account without a password (made by the single sign-on of older
    // versions) is checked against the dummy hash, it takes as long and
    // always fails
    let stored = user
        .as_ref()
        .map(|u| u.password_hash.clone())
        .filter(|h| !h.is_empty());
    let has_password = stored.is_some();
    let check = check_password(&state, req.password, stored).await?;
    let ip_text = ip.0.to_string();
    let user = match (user, check == Check::Bad || !has_password) {
        (Some(user), false) => user,
        (user, _) => {
            failed_sign_in(&state, user.map(|u| u.id), name, ip, now).await?;
            return Err(bad_credentials());
        }
    };
    not_disabled(&user)?;
    let device = device_info(&req.device);
    if let Check::GoodRehash(new_hash) = check {
        let id = user.id.clone();
        state
            .db
            .call(move |c| store::set_password(c, &id, &new_hash))
            .await?;
    }
    if let Some(needed) = second_factor::ask(&state, &user, device.clone(), now).await? {
        return Ok((StatusCode::ACCEPTED, Json(needed)).into_response());
    }
    let session = finish_sign_in(&state, user, device, &ip_text, None).await?;
    Ok(Json(session).into_response())
}

/// Refuses while sign-ins to the account, or to the name when no account
/// has it, are locked for someone at `ip`, saying for how long.
pub(super) async fn not_locked(
    state: &AppState,
    user: Option<&UserRow>,
    name: &str,
    ip: ClientIp,
    now: i64,
) -> Result<(), Error> {
    let id = user.map(|u| u.id.clone());
    let name = name.to_owned();
    let end = state
        .db
        .call(move |c| store::lock_end(c, subject(id.as_deref(), &name), ip.0, now))
        .await?;
    match end {
        Some(until) => {
            let seconds = u64::try_from((until - now + 999) / 1000).unwrap_or(1);
            Err(Error::new(
                StatusCode::TOO_MANY_REQUESTS,
                error_code::LOCKED_OUT,
                "too many failed sign-ins",
            )
            .retry_after(seconds))
        }
        None => Ok(()),
    }
}

/// The account, or else the name, sign-in failures count for.
fn subject<'a>(id: Option<&'a str>, name: &'a str) -> store::Subject<'a> {
    id.map_or(store::Subject::Name(name), store::Subject::Account)
}

pub(super) fn not_disabled(user: &UserRow) -> Result<(), Error> {
    if user.disabled_at.is_some() {
        return Err(Error::new(
            StatusCode::FORBIDDEN,
            error_code::ACCOUNT_DISABLED,
            "this account is turned off",
        ));
    }
    Ok(())
}

/// The last step of every way in: clears the failure count, signs the
/// device in and logs how.
pub(super) async fn finish_sign_in(
    state: &AppState,
    user: UserRow,
    device: DeviceInfo,
    ip: &str,
    how: Option<String>,
) -> Result<Session, Error> {
    let now = state.clock.now();
    let id = user.id.clone();
    let auth = state.config.auth.clone();
    let ip = ip.to_owned();
    let tokens = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            store::record_success(&tx, &id)?;
            let tokens = store::create_session(&tx, &id, &device, now, &auth)?;
            store::audit(
                &tx,
                now,
                &Event {
                    name: "sign_in",
                    user_id: Some(&id),
                    device_id: Some(&tokens.device_id),
                    ip: Some(&ip),
                    detail: how.as_deref(),
                },
            )?;
            tx.commit()?;
            Ok(tokens)
        })
        .await?;
    Ok(session(&user, tokens))
}

/// Counts a failure against the account, locking it when there are too
/// many, and logs it. An unknown username is counted and logged too, so the
/// answer takes as long, and locks the same way, as for a real account:
/// nobody learns which usernames exist.
pub(super) async fn failed_sign_in(
    state: &AppState,
    user_id: Option<String>,
    name: String,
    ip: ClientIp,
    now: i64,
) -> Result<(), Error> {
    let auth = state.config.auth.clone();
    state
        .db
        .call(move |c| {
            let ip_text = ip.0.to_string();
            let event = |name| Event {
                name,
                user_id: user_id.as_deref(),
                ip: Some(&ip_text),
                detail: user_id.is_none().then_some("no such account"),
                ..Event::default()
            };
            let locked =
                store::record_failure(c, subject(user_id.as_deref(), &name), ip.0, now, &auth)?;
            store::audit(c, now, &event("sign_in_failed"))?;
            if locked.is_some() && user_id.is_some() {
                store::audit(c, now, &event("locked_out"))?;
            }
            Ok(())
        })
        .await?;
    Ok(())
}

pub(super) fn bad_credentials() -> Error {
    Error::new(
        StatusCode::UNAUTHORIZED,
        error_code::BAD_CREDENTIALS,
        "wrong username or password",
    )
}

/// `POST /api/v1/auth/refresh`
pub async fn refresh(
    State(state): State<AppState>,
    ip: ClientIp,
    Body(req): Body<RefreshRequest>,
) -> Result<Json<Session>, Error> {
    rate_limit(&state, ip)?;
    let now = state.clock.now();
    let hash = token::hash(req.refresh_token.trim());
    let auth = state.config.auth.clone();
    let ip_text = ip.0.to_string();
    let outcome = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let rotated = store::rotate_refresh(&tx, &hash, now, &auth)?;
            let result = match rotated {
                Ok((user_id, tokens)) => match store::user_by_id(&tx, &user_id)? {
                    Some(user) if user.disabled_at.is_none() => Ok((user, tokens)),
                    _ => {
                        store::revoke_device(&tx, &tokens.device_id, now)?;
                        Err(RefreshRefused::Invalid)
                    }
                },
                Err(refused) => {
                    if let RefreshRefused::Reused { user_id, device_id } = &refused {
                        store::audit(
                            &tx,
                            now,
                            &Event {
                                name: "token_reuse",
                                user_id: Some(user_id),
                                device_id: Some(device_id),
                                ip: Some(&ip_text),
                                detail: Some("device signed out"),
                            },
                        )?;
                    }
                    Err(refused)
                }
            };
            tx.commit()?;
            Ok(result)
        })
        .await?;
    match outcome {
        Ok((user, tokens)) => Ok(Json(session(&user, tokens))),
        Err(RefreshRefused::Reused { .. }) => Err(Error::new(
            StatusCode::UNAUTHORIZED,
            error_code::SESSION_REVOKED,
            "this sign-in was used from two places and is now closed, sign in again",
        )),
        Err(RefreshRefused::Invalid) => Err(Error::unauthorized()),
    }
}

/// `POST /api/v1/auth/logout`: signs this device out.
pub async fn logout(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
) -> Result<StatusCode, Error> {
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    state
        .db
        .call(move |c| {
            store::revoke_device(c, &me.device_id, now)?;
            store::audit(
                c,
                now,
                &Event {
                    name: "signed_out",
                    user_id: Some(&me.user.id),
                    device_id: Some(&me.device_id),
                    ip: Some(&ip_text),
                    ..Event::default()
                },
            )
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
