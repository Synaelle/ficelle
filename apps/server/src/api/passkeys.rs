// SPDX-License-Identifier: AGPL-3.0-or-later

//! Passkeys: adding and removing them, and signing in with one. Each
//! ceremony is two requests, the server state between them lives in a
//! challenge good for five minutes and one answer.

use std::sync::Arc;

use app_protocol::account::Session;
use app_protocol::error_code;
use app_protocol::security::{
    Confirm, Passkey, PasskeyAddRequest, PasskeyAdded, PasskeyCeremony, PasskeyFinishRequest,
    PasskeySignInFinish, PasskeySignInRequest,
};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use webauthn_rs::Webauthn;
use webauthn_rs::prelude::{
    AuthenticationResult, DiscoverableAuthentication, DiscoverableKey, PasskeyAuthentication,
    PasskeyRegistration, PublicKeyCredential, RegisterPublicKeyCredential, Uuid,
};

use super::auth::{finish_sign_in, not_disabled, not_locked, rate_limit};
use super::extract::{Body, ClientIp, CurrentUser};
use super::security::{confirm, log};
use crate::accounts::device_info;
use crate::accounts::factors::{self, ChallengeKind, PasskeyRow};
use crate::accounts::store::{self, Event};
use crate::accounts::{token, totp};
use crate::clock::MINUTE;
use crate::error::Error;
use crate::routes::AppState;

/// How long a ceremony may wait for the authenticator.
const CEREMONY_LIFE: i64 = 5 * MINUTE;

pub(super) fn relying_party(state: &AppState) -> Result<Arc<Webauthn>, Error> {
    state.webauthn.clone().ok_or_else(|| {
        Error::new(
            StatusCode::NOT_FOUND,
            error_code::PASSKEYS_UNAVAILABLE,
            "passkeys need server.public_url",
        )
    })
}

pub(super) fn invalid() -> Error {
    Error::new(
        StatusCode::UNAUTHORIZED,
        error_code::PASSKEY_INVALID,
        "this passkey did not work, or took too long",
    )
}

fn passkey_json(row: &PasskeyRow) -> Passkey {
    Passkey {
        id: row.id.clone(),
        name: row.name.clone(),
        created_at: row.created_at,
        last_used_at: row.last_used_at,
    }
}

/// Asks the authenticator to keep the passkey itself, with the account in
/// it, so signing in needs no username. The server's check does not depend
/// on it, only the browser reads these options.
pub(super) fn discoverable(options: &mut Value) {
    if let Some(selection) = options
        .pointer_mut("/publicKey/authenticatorSelection")
        .and_then(Value::as_object_mut)
    {
        // browsers follow residentKey, the older requireResidentKey stays
        // false so authenticators that only read it still make a passkey
        selection.insert("residentKey".into(), json!("required"));
    }
}

/// The account's passkeys, for the security page.
pub(super) async fn list(state: &AppState, user_id: &str) -> Result<Vec<Passkey>, Error> {
    let id = user_id.to_owned();
    let rows = state.db.call(move |c| factors::passkeys(c, &id)).await?;
    Ok(rows.iter().map(passkey_json).collect())
}

/// The handle authenticators keep for the account: its uuid.
fn user_handle(user_id: &str) -> Result<Uuid, Error> {
    Uuid::parse_str(user_id).map_err(|e| Error::internal(&e.to_string()))
}

/// Starts a check against the account's passkeys, for sign-in or as a
/// second factor. `None` when it has none.
pub(super) fn start_check(
    webauthn: &Webauthn,
    rows: &[PasskeyRow],
) -> Result<Option<(Value, PasskeyAuthentication)>, Error> {
    if rows.is_empty() {
        return Ok(None);
    }
    let creds: Vec<_> = rows.iter().map(|r| r.passkey.clone()).collect();
    let (options, check) = webauthn
        .start_passkey_authentication(&creds)
        .map_err(|e| Error::internal(&e.to_string()))?;
    let options = serde_json::to_value(options).map_err(|e| Error::internal(&e.to_string()))?;
    Ok(Some((options, check)))
}

/// Checks an answer and returns the passkey with its counter moved on.
/// `None` when the answer does not check out.
pub(super) fn finish_check(
    webauthn: &Webauthn,
    rows: &[PasskeyRow],
    check: &PasskeyAuthentication,
    answer: Value,
) -> Option<webauthn_rs::prelude::Passkey> {
    let answer: PublicKeyCredential = serde_json::from_value(answer).ok()?;
    let result: AuthenticationResult = webauthn
        .finish_passkey_authentication(&answer, check)
        .ok()?;
    let row = rows
        .iter()
        .find(|r| r.passkey.cred_id() == result.cred_id())?;
    let mut passkey = row.passkey.clone();
    passkey.update_credential(&result);
    Some(passkey)
}

/// `POST /api/v1/me/passkeys/start`
pub async fn add_start(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(req): Body<PasskeyAddRequest>,
) -> Result<Json<PasskeyCeremony>, Error> {
    let webauthn = relying_party(&state)?;
    let given = Confirm {
        password: req.password,
        proof: req.proof,
    };
    confirm(&state, &me, given, ip).await?;
    let id = me.user.id.clone();
    let existing = state.db.call(move |c| factors::passkeys(c, &id)).await?;
    let exclude = existing
        .iter()
        .map(|r| r.passkey.cred_id().clone())
        .collect();
    let (options, registration) = webauthn
        .start_passkey_registration(
            user_handle(&me.user.id)?,
            &me.user.username,
            &me.user.display_name,
            Some(exclude),
        )
        .map_err(|e| Error::internal(&e.to_string()))?;
    let mut options = serde_json::to_value(options).map_err(|e| Error::internal(&e.to_string()))?;
    discoverable(&mut options);
    let expires_at = state.clock.now() + CEREMONY_LIFE;
    let id = me.user.id.clone();
    let ceremony = state
        .db
        .call(move |c| {
            factors::create_challenge(
                c,
                ChallengeKind::PasskeyAdd,
                Some(&id),
                &registration,
                expires_at,
            )
        })
        .await?;
    Ok(Json(PasskeyCeremony {
        id: ceremony,
        options,
    }))
}

/// `POST /api/v1/me/passkeys/finish`
pub async fn add_finish(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(req): Body<PasskeyFinishRequest>,
) -> Result<(StatusCode, Json<PasskeyAdded>), Error> {
    let webauthn = relying_party(&state)?;
    let now = state.clock.now();
    let hash = token::hash(req.id.trim());
    let ceremony = state
        .db
        .call(move |c| {
            let found = factors::challenge::<PasskeyRegistration>(
                c,
                &hash,
                ChallengeKind::PasskeyAdd,
                now,
            )?;
            factors::end_challenge(c, &hash)?;
            Ok(found)
        })
        .await?
        .filter(|c| c.user_id.as_deref() == Some(me.user.id.as_str()))
        .ok_or_else(invalid)?;
    let answer: RegisterPublicKeyCredential =
        serde_json::from_value(req.credential).map_err(|_| invalid())?;
    let passkey = webauthn
        .finish_passkey_registration(&answer, &ceremony.data)
        .map_err(|_| invalid())?;
    let name: String = req.name.chars().filter(|c| !c.is_control()).collect();
    let name: String = name.trim().chars().take(64).collect();
    let name = if name.is_empty() {
        "Passkey".to_owned()
    } else {
        name
    };
    let id = me.user.id.clone();
    let codes = totp::new_recovery_codes();
    let hashes: Vec<_> = codes.iter().map(|(_, h)| *h).collect();
    let stored_name = name.clone();
    let added = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let Some(passkey_id) = factors::insert_passkey(&tx, &id, &stored_name, &passkey, now)?
            else {
                return Ok(None);
            };
            let fresh = factors::recovery_codes_left(&tx, &id)? == 0;
            if fresh {
                factors::replace_recovery_codes(&tx, &id, &hashes)?;
            }
            store::set_factor_required(&tx, &id, false)?;
            tx.commit()?;
            Ok(Some((passkey_id, fresh)))
        })
        .await?;
    let Some((passkey_id, fresh)) = added else {
        return Err(Error::new(
            StatusCode::CONFLICT,
            error_code::INVALID,
            "this passkey is already registered",
        ));
    };
    log(&state, &me, ip, "passkey_added").await?;
    Ok((
        StatusCode::CREATED,
        Json(PasskeyAdded {
            passkey: Passkey {
                id: passkey_id,
                name,
                created_at: now,
                last_used_at: None,
            },
            codes: if fresh {
                codes.into_iter().map(|(code, _)| code).collect()
            } else {
                Vec::new()
            },
        }),
    ))
}

/// `DELETE /api/v1/me/passkeys/{id}`
pub async fn remove(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Path(passkey): Path<String>,
    Body(req): Body<Confirm>,
) -> Result<StatusCode, Error> {
    confirm(&state, &me, req, ip).await?;
    let id = me.user.id.clone();
    let removed = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let removed = factors::delete_passkey(&tx, &id, &passkey)?;
            factors::drop_unneeded_recovery_codes(&tx, &id)?;
            tx.commit()?;
            Ok(removed)
        })
        .await?;
    if !removed {
        return Err(Error::not_found());
    }
    log(&state, &me, ip, "passkey_removed").await?;
    Ok(StatusCode::NO_CONTENT)
}

/// What a passkey sign-in keeps between its two requests. No check for an
/// unknown username or an account without passkeys: the answer is the same
/// shape, it just never works, so nobody learns which accounts exist.
/// Without a username the browser offers the passkeys it holds and the
/// answer names the account.
#[derive(Debug, Serialize, Deserialize)]
struct SignIn {
    check: Option<PasskeyAuthentication>,
    #[serde(default)]
    discoverable: Option<DiscoverableAuthentication>,
}

/// `POST /api/v1/auth/passkey/start` without a username.
async fn start_discoverable(
    state: &AppState,
    webauthn: &Webauthn,
) -> Result<Json<PasskeyCeremony>, Error> {
    let (options, check) = webauthn
        .start_discoverable_authentication()
        .map_err(|e| Error::internal(&e.to_string()))?;
    let mut options = serde_json::to_value(options).map_err(|e| Error::internal(&e.to_string()))?;
    // the library asks for the autofill prompt, the page opens the browser's
    // own passkey dialog from a button instead
    if let Some(o) = options.as_object_mut() {
        o.remove("mediation");
    }
    let expires_at = state.clock.now() + CEREMONY_LIFE;
    let data = SignIn {
        check: None,
        discoverable: Some(check),
    };
    let ceremony = state
        .db
        .call(move |c| {
            factors::create_challenge(c, ChallengeKind::PasskeySignIn, None, &data, expires_at)
        })
        .await?;
    Ok(Json(PasskeyCeremony {
        id: ceremony,
        options,
    }))
}

/// Checks a discoverable answer against the account's passkeys and returns
/// the passkey used, its counter moved on.
fn finish_discoverable(
    webauthn: &Webauthn,
    rows: &[PasskeyRow],
    check: DiscoverableAuthentication,
    answer: &PublicKeyCredential,
) -> Option<webauthn_rs::prelude::Passkey> {
    let keys: Vec<DiscoverableKey> = rows.iter().map(|r| (&r.passkey).into()).collect();
    let result = webauthn
        .finish_discoverable_authentication(answer, check, &keys)
        .ok()?;
    let row = rows
        .iter()
        .find(|r| r.passkey.cred_id() == result.cred_id())?;
    let mut passkey = row.passkey.clone();
    passkey.update_credential(&result);
    Some(passkey)
}

/// `POST /api/v1/auth/passkey/start`
pub async fn sign_in_start(
    State(state): State<AppState>,
    ip: ClientIp,
    Body(req): Body<PasskeySignInRequest>,
) -> Result<Json<PasskeyCeremony>, Error> {
    rate_limit(&state, ip)?;
    let webauthn = relying_party(&state)?;
    let name = req.username.trim().to_ascii_lowercase();
    if name.is_empty() {
        return start_discoverable(&state, &webauthn).await;
    }
    let found = state
        .db
        .call(move |c| {
            let Some(user) = store::user_by_name(c, &name)? else {
                return Ok(None);
            };
            let rows = factors::passkeys(c, &user.id)?;
            Ok(Some((user.id, rows)))
        })
        .await?;
    let (user_id, rows) = found.unzip();
    let started = start_check(&webauthn, &rows.unwrap_or_default())?;
    let (options, check) = match started {
        Some((options, check)) => (options, Some(check)),
        None => (decoy_options(&state), None),
    };
    let expires_at = state.clock.now() + CEREMONY_LIFE;
    let ceremony = state
        .db
        .call(move |c| {
            factors::create_challenge(
                c,
                ChallengeKind::PasskeySignIn,
                user_id.as_deref(),
                &SignIn {
                    check,
                    discoverable: None,
                },
                expires_at,
            )
        })
        .await?;
    Ok(Json(PasskeyCeremony {
        id: ceremony,
        options,
    }))
}

/// Options shaped like real ones, for accounts that cannot sign in with a
/// passkey.
fn decoy_options(state: &AppState) -> Value {
    let mut challenge = [0u8; 32];
    getrandom::fill(&mut challenge).expect("the system random source works");
    let rp_id = url::Url::parse(&state.config.server.public_url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_default();
    json!({ "publicKey": {
        "challenge": URL_SAFE_NO_PAD.encode(challenge),
        "timeout": 300_000,
        "rpId": rp_id,
        "allowCredentials": [],
        "userVerification": "required",
    }})
}

/// `POST /api/v1/auth/passkey/finish`: a passkey verifies the person on the
/// device itself, it needs no second factor.
pub async fn sign_in_finish(
    State(state): State<AppState>,
    ip: ClientIp,
    Body(req): Body<PasskeySignInFinish>,
) -> Result<Json<Session>, Error> {
    rate_limit(&state, ip)?;
    let webauthn = relying_party(&state)?;
    let now = state.clock.now();
    let hash = token::hash(req.id.trim());
    let challenge = state
        .db
        .call(move |c| {
            let challenge =
                factors::challenge::<SignIn>(c, &hash, ChallengeKind::PasskeySignIn, now)?;
            // single use, right or wrong
            if !factors::end_challenge(c, &hash)? {
                return Ok(None);
            }
            Ok(challenge)
        })
        .await?
        .ok_or_else(invalid)?;
    let answer: PublicKeyCredential =
        serde_json::from_value(req.credential.clone()).map_err(|_| invalid())?;
    let (user_id, data) = match (challenge.user_id, challenge.data) {
        (Some(id), data @ SignIn { check: Some(_), .. }) => (id, data),
        (
            None,
            data @ SignIn {
                discoverable: Some(_),
                ..
            },
        ) => {
            let (handle, _) = webauthn
                .identify_discoverable_authentication(&answer)
                .map_err(|_| invalid())?;
            (handle.to_string(), data)
        }
        _ => return Err(invalid()),
    };
    let found = state
        .db
        .call(move |c| {
            let Some(user) = store::user_by_id(c, &user_id)? else {
                return Ok(None);
            };
            let rows = factors::passkeys(c, &user.id)?;
            Ok(Some((user, rows)))
        })
        .await?;
    let ip_text = ip.0.to_string();
    let Some((user, rows)) = found else {
        return Err(invalid());
    };
    not_locked(&user, now)?;
    not_disabled(&user)?;
    let checked = match data {
        SignIn {
            check: Some(check), ..
        } => finish_check(&webauthn, &rows, &check, req.credential),
        SignIn {
            discoverable: Some(check),
            ..
        } => finish_discoverable(&webauthn, &rows, check, &answer),
        SignIn { .. } => None,
    };
    let Some(passkey) = checked else {
        let id = user.id.clone();
        let ip = ip_text.clone();
        state
            .db
            .call(move |c| {
                store::audit(
                    c,
                    now,
                    &Event {
                        name: "passkey_failed",
                        user_id: Some(&id),
                        ip: Some(&ip),
                        ..Event::default()
                    },
                )
            })
            .await?;
        return Err(invalid());
    };
    state
        .db
        .call(move |c| factors::passkey_used(c, &passkey, now))
        .await?;
    let session = finish_sign_in(
        &state,
        user,
        device_info(&req.device),
        &ip_text,
        Some("passkey".into()),
    )
    .await?;
    Ok(Json(session))
}
