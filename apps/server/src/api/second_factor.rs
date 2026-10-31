// SPDX-License-Identifier: AGPL-3.0-or-later

//! The second step of a password sign-in. The password step leaves a
//! challenge good for five minutes and five answers, the second factor
//! turns it into a session. Wrong answers also count towards the account's
//! lock, so a known password does not open endless guessing.

use app_protocol::account::{DeviceInfo, Session};
use app_protocol::error_code;
use app_protocol::security::{
    EmailCodeRequest, SecondFactor, SecondFactorNeeded, SecondFactorRequest,
};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use webauthn_rs::Webauthn;
use webauthn_rs::prelude::PasskeyAuthentication;

use super::auth::{finish_sign_in, not_disabled, not_locked, rate_limit};
use super::extract::{Body, ClientIp};
use super::passkeys;
use crate::accounts::factors::{self, ChallengeKind};
use crate::accounts::store::{self, Event, UserRow};
use crate::accounts::{token, totp};
use crate::clock::MINUTE;
use crate::db::DbError;
use crate::error::Error;
use crate::routes::AppState;

/// How long the second step may wait.
const CHALLENGE_LIFE: i64 = 5 * MINUTE;
/// Wrong answers before the challenge is dropped.
const MAX_ATTEMPTS: i64 = 5;
/// How long a code sent by email works.
const EMAIL_CODE_LIFE: i64 = 10 * MINUTE;

/// What the password step keeps for the second.
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Pending {
    device: DeviceInfo,
    /// Passkey check started with the challenge.
    passkey: Option<PasskeyAuthentication>,
}

/// The factors an account signs in with after its password, `None` when it
/// has none and the password is enough.
pub(super) async fn ask(
    state: &AppState,
    user: &UserRow,
    device: DeviceInfo,
    now: i64,
) -> Result<Option<SecondFactorNeeded>, Error> {
    let id = user.id.clone();
    let expires_at = now + CHALLENGE_LIFE;
    let rows = state.db.call(move |c| factors::passkeys(c, &id)).await?;
    let started = match &state.webauthn {
        Some(webauthn) => passkeys::start_check(webauthn, &rows)?,
        None => None,
    };
    let (options, check) = started.unzip();
    let id = user.id.clone();
    let mail = state.mailer.enabled();
    let found = state
        .db
        .call(move |c| {
            let totp = factors::totp(c, &id)?.secret.is_some();
            let mut methods = Vec::new();
            if check.is_some() {
                methods.push(SecondFactor::Passkey);
            }
            if totp {
                methods.push(SecondFactor::Totp);
            }
            // the weakest last, and only while mail goes out
            if mail && factors::email_codes(c, &id)?.is_some() {
                methods.push(SecondFactor::Email);
            }
            if methods.is_empty() {
                return Ok(None);
            }
            if factors::recovery_codes_left(c, &id)? > 0 {
                methods.push(SecondFactor::RecoveryCode);
            }
            let challenge = factors::create_challenge(
                c,
                ChallengeKind::SecondFactor,
                Some(&id),
                &Pending {
                    device,
                    passkey: check,
                },
                expires_at,
            )?;
            Ok(Some((challenge, methods)))
        })
        .await?;
    Ok(found.map(|(challenge, methods)| SecondFactorNeeded {
        challenge,
        methods,
        expires_at,
        passkey: options,
    }))
}

fn invalid() -> Error {
    Error::new(
        StatusCode::UNAUTHORIZED,
        error_code::SECOND_FACTOR_INVALID,
        "this code does not work, or the sign-in took too long",
    )
}

/// A second factor as sent.
struct Answer<'a> {
    /// The hash of the sign-in's challenge, which a code by email is kept by.
    challenge: &'a token::TokenHash,
    method: SecondFactor,
    code: &'a str,
    webauthn: Option<&'a Webauthn>,
    check: Option<&'a PasskeyAuthentication>,
    passkey: Option<serde_json::Value>,
}

/// Checks and spends a factor. How it passed, for the log, or `None`.
fn check_answer(
    tx: &rusqlite::Connection,
    master: &crate::keys::MasterKey,
    id: &str,
    answer: Answer<'_>,
    now: i64,
) -> Result<Option<String>, DbError> {
    Ok(match answer.method {
        SecondFactor::Totp => {
            let state = factors::totp(tx, id)?;
            let step = state
                .secret
                .as_deref()
                .and_then(|s| factors::open_totp(master, id, s))
                .and_then(|s| totp::check(&s, answer.code, now, state.last_step));
            if let Some(step) = step {
                factors::used_totp_step(tx, id, step)?;
            }
            step.map(|_| "authenticator app".to_owned())
        }
        SecondFactor::RecoveryCode => {
            let hash = totp::hash_recovery_code(answer.code);
            if factors::use_recovery_code(tx, id, &hash, now)? {
                let left = factors::recovery_codes_left(tx, id)?;
                Some(format!("recovery code, {left} left"))
            } else {
                None
            }
        }
        SecondFactor::Passkey => {
            let rows = factors::passkeys(tx, id)?;
            let checked = match (answer.webauthn, answer.check, answer.passkey) {
                (Some(webauthn), Some(check), Some(passkey)) => {
                    passkeys::finish_check(webauthn, &rows, check, passkey)
                }
                _ => None,
            };
            if let Some(passkey) = &checked {
                factors::passkey_used(tx, passkey, now)?;
            }
            checked.map(|_| "passkey".to_owned())
        }
        SecondFactor::Email => factors::use_email_code(tx, answer.challenge, answer.code, now)?
            .then(|| "code by email".to_owned()),
    })
}

/// `POST /api/v1/auth/second-factor`
pub async fn second_factor(
    State(state): State<AppState>,
    ip: ClientIp,
    Body(req): Body<SecondFactorRequest>,
) -> Result<Json<Session>, Error> {
    rate_limit(&state, ip)?;
    let now = state.clock.now();
    let hash = token::hash(req.challenge.trim());
    let found = state
        .db
        .call(move |c| {
            let Some(challenge) =
                factors::challenge::<Pending>(c, &hash, ChallengeKind::SecondFactor, now)?
            else {
                return Ok(None);
            };
            let user = match challenge.user_id.as_deref() {
                Some(id) => store::user_by_id(c, id)?,
                None => None,
            };
            Ok(user.map(|u| (u, challenge)))
        })
        .await?;
    let Some((user, challenge)) = found else {
        return Err(invalid());
    };
    if challenge.attempts >= MAX_ATTEMPTS {
        return Err(invalid());
    }
    not_locked(&user, now)?;
    not_disabled(&user)?;
    let ip_text = ip.0.to_string();
    let id = user.id.clone();
    let code = req.code.unwrap_or_default();
    let method = req.method;
    let Pending { device, passkey } = challenge.data;
    let webauthn = state.webauthn.clone();
    let answer = req.passkey;
    let auth = state.config.auth.clone();
    let master = state.master.clone();
    // checked and spent in one transaction: two requests racing with the
    // same code or challenge cannot both pass
    let outcome = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let answer = Answer {
                challenge: &hash,
                method,
                code: &code,
                webauthn: webauthn.as_deref(),
                check: passkey.as_ref(),
                passkey: answer,
            };
            let passed = check_answer(&tx, &master, &id, answer, now)?;
            let outcome = match passed {
                Some(how) if factors::end_challenge(&tx, &hash)? => Some(how),
                // another request with the same challenge got there first
                Some(_) => None,
                None => {
                    let attempts = factors::challenge_failed(&tx, &hash)?;
                    if attempts >= MAX_ATTEMPTS {
                        factors::end_challenge(&tx, &hash)?;
                    }
                    let locked = store::record_failure(&tx, &id, now, &auth)?;
                    let event = |name| Event {
                        name,
                        user_id: Some(&id),
                        ip: Some(&ip_text),
                        ..Event::default()
                    };
                    store::audit(&tx, now, &event("second_factor_failed"))?;
                    if locked.is_some() {
                        factors::end_challenge(&tx, &hash)?;
                        store::audit(&tx, now, &event("locked_out"))?;
                    }
                    None
                }
            };
            tx.commit()?;
            Ok(outcome)
        })
        .await?;
    let how = outcome.ok_or_else(invalid)?;
    let session = finish_sign_in(&state, user, device, &ip.0.to_string(), Some(how)).await?;
    Ok(Json(session))
}

/// `POST /api/v1/auth/second-factor/email`: sends a code to the account
/// signing in, when codes by email are its second step. Answers the same
/// whether or not a mail went, a few times per sign-in at most.
pub async fn email_code(
    State(state): State<AppState>,
    ip: ClientIp,
    super::extract::Language(lang): super::extract::Language,
    Body(req): Body<EmailCodeRequest>,
) -> Result<StatusCode, Error> {
    rate_limit(&state, ip)?;
    if !state.mailer.enabled() {
        return Err(invalid());
    }
    let now = state.clock.now();
    let hash = token::hash(req.challenge.trim());
    let code = totp::email_code();
    let kept = code.clone();
    let found = state
        .db
        .call(move |c| {
            let Some(challenge) =
                factors::challenge::<Pending>(c, &hash, ChallengeKind::SecondFactor, now)?
            else {
                return Ok(None);
            };
            let Some(id) = challenge.user_id else {
                return Ok(None);
            };
            let Some(user) = store::user_by_id(c, &id)? else {
                return Ok(None);
            };
            let Some(to) = factors::email_codes(c, &id)? else {
                return Ok(None);
            };
            if !factors::keep_email_code(c, &hash, &kept, now + EMAIL_CODE_LIFE)? {
                return Ok(None);
            }
            Ok(Some((to, user)))
        })
        .await?;
    let (to, user) = found.ok_or_else(invalid)?;
    let subject = crate::mail::tr(lang, "mail.code.subject", &[]);
    let body = crate::mail::tr(
        lang,
        "mail.code.body",
        &[("username", &user.username), ("code", &code)],
    );
    let greeting = crate::mail::tr(lang, "mail.greeting", &[("name", &user.display_name)]);
    state
        .mailer
        .send(&to, subject, format!("{greeting}\n\n{body}\n"));
    Ok(StatusCode::NO_CONTENT)
}
