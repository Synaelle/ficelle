// SPDX-License-Identifier: AGPL-3.0-or-later

//! End-to-end encrypted accounts: sign-up and sign-in over OPAQUE, proofs
//! of the password for sensitive changes, the wrapped keys, a new password
//! and recovery with the recovery code. The password never reaches the
//! server, and the server cannot open the keys it keeps.

use std::sync::Arc;

use app_e2ee::server::Setup;
use app_protocol::account::{Role, Session};
use app_protocol::e2ee::{
    AccountKeys, AccountKind, AccountKinds, ConfirmFinish, E2eePasswordRequest,
    E2eeRegisterRequest, KeyBundle, KindAnswer, KindRequest, OpaqueLoginFinish, OpaqueLoginStart,
    OpaqueLoginStarted, OpaqueRegistrationRequest, OpaqueRegistrationResponse, Proof,
    RecoveryFinish, RecoveryStart, RecoveryStarted,
};
use app_protocol::error_code;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use super::auth::{
    admission, bad_credentials, failed_sign_in, finish_sign_in, not_disabled, not_locked,
    rate_limit, session, taken,
};
use super::extract::{Body, ClientIp, CurrentUser};
use super::second_factor;
use crate::accounts::factors::{self, ChallengeKind};
use crate::accounts::store::{self, Event, UserRow};
use crate::accounts::{device_info, display_name, e2ee, normal_username, token};
use crate::clock::MINUTE;
use crate::error::Error;
use crate::routes::AppState;

/// How long a sign-in, a proof or a recovery may wait.
const STEP_LIFE: i64 = 5 * MINUTE;

/// The server's OPAQUE setup, read or made once.
pub(super) async fn setup(state: &AppState) -> Result<Arc<Setup>, Error> {
    let db = state.db.clone();
    let master = state.master.clone();
    state
        .opaque
        .get_or_try_init(|| async move {
            db.call(move |c| e2ee::setup(c, &master).map(Arc::new))
                .await
                .map_err(Error::from)
        })
        .await
        .cloned()
}

fn malformed(err: &app_e2ee::Error) -> Error {
    Error::invalid(err.to_string())
}

fn username(raw: &str) -> String {
    raw.trim().to_ascii_lowercase()
}

/// `POST /api/v1/auth/kind`. An unknown username gets the kind sign-up
/// creates, or under a choice a kind drawn from the username with a server
/// secret: stable, and no different from a real account's answer.
pub async fn kind(
    State(state): State<AppState>,
    ip: ClientIp,
    Body(req): Body<KindRequest>,
) -> Result<Json<KindAnswer>, Error> {
    rate_limit(&state, ip)?;
    let name = username(&req.username);
    let lookup = name.clone();
    let (user, kinds) = state
        .db
        .call(move |c| Ok((store::user_by_name(c, &lookup)?, e2ee::account_kinds(c)?)))
        .await?;
    let kind = match (user, kinds) {
        (Some(user), _) => user.kind,
        (None, AccountKinds::Standard) => AccountKind::Standard,
        (None, AccountKinds::E2ee) => AccountKind::E2ee,
        (None, AccountKinds::Choice) => {
            let secret = setup(&state).await?.to_bytes();
            let mut mac =
                Hmac::<Sha256>::new_from_slice(&secret).expect("hmac takes any key length");
            mac.update(b"decoy account kind\0");
            mac.update(name.as_bytes());
            if mac.finalize().into_bytes()[0] & 1 == 0 {
                AccountKind::Standard
            } else {
                AccountKind::E2ee
            }
        }
    };
    Ok(Json(KindAnswer { kind }))
}

/// `POST /api/v1/auth/opaque/registration`. Keeps nothing: the answer only
/// depends on the server setup and the username, which anyone can ask for.
pub async fn registration(
    State(state): State<AppState>,
    ip: ClientIp,
    Body(req): Body<OpaqueRegistrationRequest>,
) -> Result<Json<OpaqueRegistrationResponse>, Error> {
    rate_limit(&state, ip)?;
    let setup = setup(&state).await?;
    let response = setup
        .registration_response(&req.request, &username(&req.username))
        .map_err(|e| malformed(&e))?;
    Ok(Json(OpaqueRegistrationResponse { response }))
}

/// A stretching number from a request: one this server knows.
fn ksf(number: u8) -> Result<u8, Error> {
    if (1..=app_protocol::e2ee::KSF_CURRENT).contains(&number) {
        Ok(number)
    } else {
        Err(Error::invalid("ksf: unknown password stretching"))
    }
}

fn check_bundle(keys: &KeyBundle) -> Result<(), Error> {
    let fits = |s: &str| !s.is_empty() && s.len() <= 1024;
    if keys.version == 1 && fits(&keys.by_password) && fits(&keys.by_recovery) {
        Ok(())
    } else {
        Err(Error::invalid("keys: unknown format"))
    }
}

/// `POST /api/v1/auth/e2ee/register`
pub async fn register(
    State(state): State<AppState>,
    ip: ClientIp,
    Body(req): Body<E2eeRegisterRequest>,
) -> Result<(StatusCode, Json<Session>), Error> {
    rate_limit(&state, ip)?;
    let invite = admission(&state, req.invite.as_deref(), AccountKind::E2ee).await?;
    let name = normal_username(&req.username).ok_or_else(|| {
        Error::new(
            StatusCode::BAD_REQUEST,
            error_code::USERNAME_INVALID,
            "use 3 to 32 of a-z, 0-9, dot, dash and underscore, starting with a letter or digit",
        )
    })?;
    check_bundle(&req.keys)?;
    let record = Setup::record(&req.upload).map_err(|e| malformed(&e))?;
    let stretch = ksf(req.ksf)?;
    let display = display_name(req.display_name.as_deref(), &name);
    let device = device_info(&req.device);
    let auth = state.config.auth.clone();
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    let created = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let Some(user) = store::insert_user(&tx, &name, &display, "", Role::User, now)? else {
                return Ok(Err(taken()));
            };
            e2ee::make_e2ee(
                &tx,
                &user.id,
                &record,
                stretch,
                &req.keys,
                &req.recovery_verifier,
            )?;
            if let Some(h) = invite
                && !store::use_invite(&tx, &h, &user.id, now)?
            {
                return Ok(Err(Error::new(
                    StatusCode::FORBIDDEN,
                    error_code::INVITE_INVALID,
                    "this invite is used, revoked or expired",
                )));
            }
            let tokens = store::create_session(&tx, &user.id, &device, now, &auth)?;
            let detail = if invite.is_some() {
                "e2ee, invite"
            } else {
                "e2ee, open"
            };
            store::audit(
                &tx,
                now,
                &Event {
                    name: "registered",
                    user_id: Some(&user.id),
                    device_id: Some(&tokens.device_id),
                    ip: Some(&ip_text),
                    detail: Some(detail),
                },
            )?;
            let user = store::user_by_id(&tx, &user.id)?.unwrap_or(user);
            tx.commit()?;
            Ok(Ok((user, tokens)))
        })
        .await??;
    let (user, tokens) = created;
    Ok((StatusCode::CREATED, Json(session(&user, tokens))))
}

/// What a sign-in keeps between its two requests.
#[derive(Debug, Serialize, Deserialize)]
struct LoginState {
    username: String,
    /// The server's OPAQUE state, base64.
    state: String,
}

/// Starts an OPAQUE sign-in as `name`, as a challenge of `kind`. Accounts
/// that are not e2ee, and unknown names, get an answer of the same shape
/// that never finishes.
async fn start_login(
    state: &AppState,
    name: String,
    request: &str,
    kind: ChallengeKind,
) -> Result<OpaqueLoginStarted, Error> {
    let now = state.clock.now();
    let lookup = name.clone();
    let (user, record, old_remain) = state
        .db
        .call(move |c| {
            let user = store::user_by_name(c, &lookup)?;
            let record = match &user {
                Some(u) => e2ee::record(c, &u.id)?,
                None => None,
            };
            let old_remain = record.is_none() && e2ee::old_records_remain(c)?;
            Ok((user, record, old_remain))
        })
        .await?;
    if let Some(user) = &user {
        not_locked(user, now)?;
    }
    let setup = setup(state).await?;
    let ksf = match &record {
        Some((_, ksf)) => *ksf,
        // no record: a number like an account's, stable for the name, so it
        // does not tell an old account from none
        None if old_remain => decoy_ksf(&setup, &name),
        None => app_protocol::e2ee::KSF_CURRENT,
    };
    let started = setup
        .login_start(record.as_ref().map(|(r, _)| r.as_slice()), request, &name)
        .map_err(|e| malformed(&e))?;
    let data = LoginState {
        username: name,
        state: app_e2ee::encode(&started.state),
    };
    let user_id = user.filter(|u| u.kind == AccountKind::E2ee).map(|u| u.id);
    let challenge = state
        .db
        .call(move |c| {
            factors::create_challenge(c, kind, user_id.as_deref(), &data, now + STEP_LIFE)
        })
        .await?;
    Ok(OpaqueLoginStarted {
        challenge,
        response: started.response,
        ksf,
    })
}

/// The stretching number an unknown username answers while old records
/// remain: drawn from the name with the server secret, like its kind.
fn decoy_ksf(setup: &Setup, name: &str) -> u8 {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(&setup.to_bytes()).expect("hmac takes any key length");
    mac.update(b"decoy ksf\0");
    mac.update(name.as_bytes());
    if mac.finalize().into_bytes()[0] & 1 == 0 {
        1
    } else {
        app_protocol::e2ee::KSF_CURRENT
    }
}

/// Finishes an OPAQUE sign-in: the account when the password was right.
/// `None` for a wrong password or an unknown account, after counting the
/// failure.
async fn finish_login(
    state: &AppState,
    challenge: &str,
    finalization: &str,
    kind: ChallengeKind,
    ip: ClientIp,
) -> Result<Option<UserRow>, Error> {
    let now = state.clock.now();
    let hash = token::hash(challenge.trim());
    let found = state
        .db
        .call(move |c| {
            let found = factors::challenge::<LoginState>(c, &hash, kind, now)?;
            // single use, right or wrong
            if !factors::end_challenge(c, &hash)? {
                return Ok(None);
            }
            let Some(found) = found else {
                return Ok(None);
            };
            let user = match found.user_id.as_deref() {
                Some(id) => store::user_by_id(c, id)?,
                None => None,
            };
            Ok(Some((found.data, user)))
        })
        .await?;
    let Some((login, user)) = found else {
        return Err(bad_credentials());
    };
    let server_state = app_e2ee::decode(&login.state, "login state").map_err(|e| malformed(&e))?;
    let passed =
        user.is_some() && Setup::login_finish(&server_state, finalization, &login.username).is_ok();
    if passed {
        return Ok(user);
    }
    failed_sign_in(state, user.map(|u| u.id), ip.0.to_string(), now).await?;
    Ok(None)
}

/// `POST /api/v1/auth/opaque/login/start`
pub async fn login_start(
    State(state): State<AppState>,
    ip: ClientIp,
    Body(req): Body<OpaqueLoginStart>,
) -> Result<Json<OpaqueLoginStarted>, Error> {
    rate_limit(&state, ip)?;
    let started = start_login(
        &state,
        username(&req.username),
        &req.request,
        ChallengeKind::OpaqueLogin,
    )
    .await?;
    Ok(Json(started))
}

/// `POST /api/v1/auth/opaque/login/finish`: a session, or 202 and a second
/// factor challenge.
pub async fn login_finish(
    State(state): State<AppState>,
    ip: ClientIp,
    Body(req): Body<OpaqueLoginFinish>,
) -> Result<Response, Error> {
    rate_limit(&state, ip)?;
    let user = finish_login(
        &state,
        &req.challenge,
        &req.finalization,
        ChallengeKind::OpaqueLogin,
        ip,
    )
    .await?
    .ok_or_else(bad_credentials)?;
    not_disabled(&user)?;
    let now = state.clock.now();
    let device = device_info(&req.device);
    if let Some(needed) = second_factor::ask(&state, &user, device.clone(), now).await? {
        return Ok((StatusCode::ACCEPTED, Json(needed)).into_response());
    }
    let session = finish_sign_in(&state, user, device, &ip.0.to_string(), None).await?;
    Ok(Json(session).into_response())
}

fn not_e2ee() -> Error {
    Error::new(
        StatusCode::CONFLICT,
        error_code::INVALID,
        "this account is not end-to-end encrypted",
    )
}

/// `POST /api/v1/me/confirm/start`: an OPAQUE sign-in as the signed in
/// account, whose only result is a proof of the password.
pub async fn confirm_start(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(req): Body<OpaqueLoginStart>,
) -> Result<Json<OpaqueLoginStarted>, Error> {
    rate_limit(&state, ip)?;
    if me.user.kind != AccountKind::E2ee {
        return Err(not_e2ee());
    }
    let started = start_login(
        &state,
        me.user.username.clone(),
        &req.request,
        ChallengeKind::OpaqueConfirm,
    )
    .await?;
    Ok(Json(started))
}

/// `POST /api/v1/me/confirm/finish`
pub async fn confirm_finish(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(req): Body<ConfirmFinish>,
) -> Result<Json<Proof>, Error> {
    rate_limit(&state, ip)?;
    let user = finish_login(
        &state,
        &req.challenge,
        &req.finalization,
        ChallengeKind::OpaqueConfirm,
        ip,
    )
    .await?
    .filter(|u| u.id == me.user.id)
    .ok_or_else(|| {
        Error::new(
            StatusCode::UNAUTHORIZED,
            error_code::BAD_CREDENTIALS,
            "the password is wrong",
        )
    })?;
    let now = state.clock.now();
    let proof = state
        .db
        .call(move |c| {
            store::record_success(c, &user.id)?;
            factors::create_challenge(
                c,
                ChallengeKind::ConfirmProof,
                Some(&user.id),
                &(),
                now + STEP_LIFE,
            )
        })
        .await?;
    Ok(Json(Proof { proof }))
}

/// Spends a proof of the account's password. `false` when it is unknown,
/// used, expired or another account's.
pub(super) async fn spend_proof(
    state: &AppState,
    user_id: &str,
    proof: &str,
) -> Result<bool, Error> {
    let now = state.clock.now();
    let hash = token::hash(proof.trim());
    let user_id = user_id.to_owned();
    Ok(state
        .db
        .call(move |c| {
            let found = factors::challenge::<()>(c, &hash, ChallengeKind::ConfirmProof, now)?;
            let mine = found.is_some_and(|f| f.user_id.as_deref() == Some(user_id.as_str()));
            Ok(mine && factors::end_challenge(c, &hash)?)
        })
        .await?)
}

/// `GET /api/v1/me/keys`
pub async fn keys(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
) -> Result<Json<AccountKeys>, Error> {
    let id = me.user.id.clone();
    let keys = state
        .db
        .call(move |c| e2ee::keys(c, &id))
        .await?
        .ok_or_else(not_e2ee)?;
    Ok(Json(AccountKeys { keys }))
}

/// The recovery wrapping only changes with a new recovery code, which this
/// version does not offer: a bundle that touches it is refused.
fn same_recovery(stored: &KeyBundle, new: &KeyBundle) -> Result<(), Error> {
    check_bundle(new)?;
    if stored.by_recovery == new.by_recovery {
        Ok(())
    } else {
        Err(Error::invalid(
            "keys: the recovery wrapping cannot change here",
        ))
    }
}

/// `POST /api/v1/me/e2ee/password`
pub async fn change_password(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(req): Body<E2eePasswordRequest>,
) -> Result<StatusCode, Error> {
    if me.user.kind != AccountKind::E2ee {
        return Err(not_e2ee());
    }
    if !spend_proof(&state, &me.user.id, &req.proof).await? {
        return Err(Error::new(
            StatusCode::FORBIDDEN,
            error_code::CONFIRM_NEEDED,
            "confirm the current password first",
        ));
    }
    let record = Setup::record(&req.upload).map_err(|e| malformed(&e))?;
    let stretch = ksf(req.ksf)?;
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let Some(stored) = e2ee::keys(&tx, &me.user.id)? else {
                return Ok(Err(not_e2ee()));
            };
            if let Err(e) = same_recovery(&stored, &req.keys) {
                return Ok(Err(e));
            }
            e2ee::set_password(&tx, &me.user.id, &record, stretch, &req.keys)?;
            let others = store::revoke_other_devices(&tx, &me.user.id, Some(&me.device_id), now)?;
            let detail = format!("{others} other devices signed out");
            store::audit(
                &tx,
                now,
                &Event {
                    name: "password_changed",
                    user_id: Some(&me.user.id),
                    device_id: Some(&me.device_id),
                    ip: Some(&ip_text),
                    detail: Some(&detail),
                },
            )?;
            tx.commit()?;
            Ok(Ok(()))
        })
        .await??;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/v1/me/e2ee/strengthen`: the same password registered again
/// with stronger stretching, by an app that just signed in with it. Needs
/// a proof of the password like a change, signs nobody out, and only ever
/// makes the stretching stronger.
pub async fn strengthen(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(req): Body<E2eePasswordRequest>,
) -> Result<StatusCode, Error> {
    if me.user.kind != AccountKind::E2ee {
        return Err(not_e2ee());
    }
    let stretch = ksf(req.ksf)?;
    if !spend_proof(&state, &me.user.id, &req.proof).await? {
        return Err(Error::new(
            StatusCode::FORBIDDEN,
            error_code::CONFIRM_NEEDED,
            "confirm the current password first",
        ));
    }
    let record = Setup::record(&req.upload).map_err(|e| malformed(&e))?;
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let (Some(stored), Some((_, current))) = (
                e2ee::keys(&tx, &me.user.id)?,
                e2ee::record(&tx, &me.user.id)?,
            ) else {
                return Ok(Err(not_e2ee()));
            };
            if stretch <= current {
                return Ok(Err(Error::new(
                    StatusCode::CONFLICT,
                    error_code::INVALID,
                    "the password is already stretched this much",
                )));
            }
            if let Err(e) = same_recovery(&stored, &req.keys) {
                return Ok(Err(e));
            }
            e2ee::set_password(&tx, &me.user.id, &record, stretch, &req.keys)?;
            let detail = format!("stretching {current} to {stretch}");
            store::audit(
                &tx,
                now,
                &Event {
                    name: "password_strengthened",
                    user_id: Some(&me.user.id),
                    device_id: Some(&me.device_id),
                    ip: Some(&ip_text),
                    detail: Some(&detail),
                },
            )?;
            tx.commit()?;
            Ok(Ok(()))
        })
        .await??;
    Ok(StatusCode::NO_CONTENT)
}

fn recovery_invalid() -> Error {
    Error::new(
        StatusCode::UNAUTHORIZED,
        error_code::RECOVERY_INVALID,
        "this recovery code does not open this account, or the recovery took too long",
    )
}

/// `POST /api/v1/auth/recovery/start`: the recovery code is checked
/// through its verifier, then the keys go out for it to open. Wrong codes
/// count towards the account lock.
pub async fn recovery_start(
    State(state): State<AppState>,
    ip: ClientIp,
    Body(req): Body<RecoveryStart>,
) -> Result<Json<RecoveryStarted>, Error> {
    rate_limit(&state, ip)?;
    let now = state.clock.now();
    let name = username(&req.username);
    let user = state
        .db
        .call(move |c| store::user_by_name(c, &name))
        .await?;
    if let Some(user) = &user {
        not_locked(user, now)?;
    }
    let user = user.filter(|u| u.kind == AccountKind::E2ee);
    let lookup = user.as_ref().map(|u| u.id.clone());
    let verifier = req.verifier;
    let granted = state
        .db
        .call(move |c| {
            let Some(id) = lookup else {
                return Ok(None);
            };
            if !e2ee::verifier_matches(c, &id, &verifier)? {
                return Ok(None);
            }
            let Some(keys) = e2ee::keys(c, &id)? else {
                return Ok(None);
            };
            let challenge = factors::create_challenge(
                c,
                ChallengeKind::Recovery,
                Some(&id),
                &(),
                now + STEP_LIFE,
            )?;
            Ok(Some((challenge, keys)))
        })
        .await?;
    let Some((challenge, keys)) = granted else {
        failed_sign_in(&state, user.map(|u| u.id), ip.0.to_string(), now).await?;
        return Err(recovery_invalid());
    };
    Ok(Json(RecoveryStarted { challenge, keys }))
}

/// `POST /api/v1/auth/recovery/finish`: the new password. The recovery
/// code is a second secret kept apart from the password, so it stands in
/// for the second factor too. Every device is signed out.
pub async fn recovery_finish(
    State(state): State<AppState>,
    ip: ClientIp,
    Body(req): Body<RecoveryFinish>,
) -> Result<Json<Session>, Error> {
    rate_limit(&state, ip)?;
    let now = state.clock.now();
    let hash = token::hash(req.challenge.trim());
    let record = Setup::record(&req.upload).map_err(|e| malformed(&e))?;
    let stretch = ksf(req.ksf)?;
    let ip_text = ip.0.to_string();
    let keys = req.keys;
    let user = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let found = factors::challenge::<()>(&tx, &hash, ChallengeKind::Recovery, now)?;
            if !factors::end_challenge(&tx, &hash)? {
                return Ok(Err(recovery_invalid()));
            }
            let Some(id) = found.and_then(|f| f.user_id) else {
                return Ok(Err(recovery_invalid()));
            };
            let Some(stored) = e2ee::keys(&tx, &id)? else {
                return Ok(Err(recovery_invalid()));
            };
            if let Err(e) = same_recovery(&stored, &keys) {
                return Ok(Err(e));
            }
            // a turned off account keeps its password
            if let Some(current) = store::user_by_id(&tx, &id)?
                && let Err(e) = not_disabled(&current)
            {
                return Ok(Err(e));
            }
            e2ee::set_password(&tx, &id, &record, stretch, &keys)?;
            let n = store::revoke_other_devices(&tx, &id, None, now)?;
            let detail = format!("{n} devices signed out");
            store::audit(
                &tx,
                now,
                &Event {
                    name: "password_recovered",
                    user_id: Some(&id),
                    ip: Some(&ip_text),
                    detail: Some(&detail),
                    ..Event::default()
                },
            )?;
            let user = store::user_by_id(&tx, &id)?;
            tx.commit()?;
            Ok(user.ok_or_else(recovery_invalid))
        })
        .await??;
    not_disabled(&user)?;
    let session = finish_sign_in(
        &state,
        user,
        device_info(&req.device),
        &ip.0.to_string(),
        Some("recovery code".into()),
    )
    .await?;
    Ok(Json(session))
}

/// How the policy reads in the security log.
pub(super) fn kinds_logged(kinds: AccountKinds) -> &'static str {
    match kinds {
        AccountKinds::Standard => "standard only",
        AccountKinds::E2ee => "end-to-end encrypted only",
        AccountKinds::Choice => "the person chooses",
    }
}
