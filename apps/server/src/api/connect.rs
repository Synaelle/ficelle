// SPDX-License-Identifier: AGPL-3.0-or-later

//! An app signing in through the server's page. With a redirect, see
//! [`crate::app_codes`]: the page asks for a code once the person agreed,
//! the app trades it for its session. With a code, see
//! [`crate::app_links`]: the app shows a code, the person types it on the
//! page and agrees, the app asks until it is approved.

use app_protocol::account::{
    AppCode, AppCodeRequest, AppLink, AppLinkCode, AppLinkInfo, AppLinkRequest,
    AppLinkTokenRequest, AppToken, AppTokenRequest,
};
use app_protocol::error_code;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;

use super::auth::{finish_sign_in, not_disabled, rate_limit};
use super::extract::{Body, ClientIp, CurrentUser};
use crate::accounts::store::{self, Event};
use crate::app_codes;
use crate::app_links::{self, Asked};
use crate::error::Error;
use crate::routes::AppState;

/// `POST /api/v1/auth/app/code`, from the page, signed in: a code for the
/// app that opened it.
pub async fn code(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(req): Body<AppCodeRequest>,
) -> Result<Json<AppCode>, Error> {
    if !app_codes::redirect_ok(&req.redirect) {
        return Err(Error::invalid(
            "the app's address is not one of this project's apps",
        ));
    }
    if !app_codes::challenge_ok(&req.challenge) {
        return Err(Error::invalid("the challenge is not an S256 challenge"));
    }
    if req
        .sealed
        .as_deref()
        .is_some_and(|s| !app_codes::sealed_ok(s))
    {
        return Err(Error::invalid("the sealed key is not one"));
    }
    if req.device.name.trim().is_empty() || req.device.platform.trim().is_empty() {
        return Err(Error::invalid("the app's device has no name"));
    }
    let now = state.clock.now();
    let ip = ip.0.to_string();
    let code = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let code = app_codes::issue(&tx, &me.user.id, &req, now)?;
            store::audit(
                &tx,
                now,
                &Event {
                    name: "app_connect",
                    user_id: Some(&me.user.id),
                    device_id: Some(&me.device_id),
                    ip: Some(&ip),
                    detail: Some(&req.device.name),
                },
            )?;
            tx.commit()?;
            Ok(code)
        })
        .await?;
    Ok(Json(AppCode { code }))
}

/// `POST /api/v1/auth/app/token`, from the app: its session for a code, the
/// verifier and the redirect it was issued for.
pub async fn token(
    State(state): State<AppState>,
    ip: ClientIp,
    Body(req): Body<AppTokenRequest>,
) -> Result<Json<AppToken>, Error> {
    rate_limit(&state, ip)?;
    let now = state.clock.now();
    let found = state
        .db
        .call(move |c| {
            let Some(redeemed) =
                app_codes::redeem(c, &req.code, &req.verifier, &req.redirect, now)?
            else {
                return Ok(None);
            };
            Ok(store::user_by_id(c, &redeemed.user_id)?.map(|user| (user, redeemed)))
        })
        .await?;
    let Some((user, redeemed)) = found else {
        return Err(Error::invalid(
            "the code is used, expired or was not made for this app",
        ));
    };
    not_disabled(&user)?;
    let session = finish_sign_in(
        &state,
        user,
        redeemed.device,
        &ip.0.to_string(),
        Some("through the server's page".to_owned()),
    )
    .await?;
    Ok(Json(AppToken {
        session,
        sealed: redeemed.sealed,
    }))
}

/// The longest device name an app may give.
const MOST_NAME: usize = 100;

fn link_invalid() -> Error {
    Error::new(
        StatusCode::BAD_REQUEST,
        error_code::LINK_INVALID,
        "the code is used, expired or was never given",
    )
}

fn typed(code: &str) -> Result<String, Error> {
    app_links::normalize(code).ok_or_else(link_invalid)
}

/// `POST /api/v1/auth/app/link`, from an app: a code to sign in with.
pub async fn link(
    State(state): State<AppState>,
    ip: ClientIp,
    Body(req): Body<AppLinkRequest>,
) -> Result<Json<AppLink>, Error> {
    rate_limit(&state, ip)?;
    if !app_codes::challenge_ok(&req.challenge) {
        return Err(Error::invalid("the challenge is not an S256 challenge"));
    }
    if !app_e2ee::public_key_ok(&req.key) {
        return Err(Error::invalid("the key is not a public key"));
    }
    let named = |s: &str| !s.trim().is_empty() && s.chars().count() <= MOST_NAME;
    if !named(&req.device.name) || !named(&req.device.platform) {
        return Err(Error::invalid("the app's device has no usable name"));
    }
    let now = state.clock.now();
    let code = state
        .db
        .call(move |c| app_links::create(c, &req, now))
        .await?
        .ok_or_else(|| {
            Error::new(
                StatusCode::TOO_MANY_REQUESTS,
                error_code::RATE_LIMITED,
                "too many apps are waiting for a code, try again in a few minutes",
            )
        })?;
    Ok(Json(AppLink {
        code: format!("{}-{}", &code[..4], &code[4..]),
        expires_at: now + app_links::LIFE,
        interval: app_links::INTERVAL,
    }))
}

/// `POST /api/v1/auth/app/link/find`, from the page: the app behind a typed
/// code, before the person signs in, so an e2ee export key can be sealed to
/// it at sign-in.
pub async fn link_find(
    State(state): State<AppState>,
    ip: ClientIp,
    Body(req): Body<AppLinkCode>,
) -> Result<Json<AppLinkInfo>, Error> {
    rate_limit(&state, ip)?;
    let code = typed(&req.code)?;
    let now = state.clock.now();
    state
        .db
        .call(move |c| app_links::find(c, &code, now))
        .await?
        .map(Json)
        .ok_or_else(link_invalid)
}

/// `POST /api/v1/auth/app/link/approve`, from the page, signed in: the
/// person agreed to connect the app behind the code.
pub async fn link_approve(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(req): Body<AppLinkCode>,
) -> Result<StatusCode, Error> {
    let code = typed(&req.code)?;
    if req
        .sealed
        .as_deref()
        .is_some_and(|s| !app_codes::sealed_ok(s))
    {
        return Err(Error::invalid("the sealed key is not one"));
    }
    let now = state.clock.now();
    let ip = ip.0.to_string();
    let approved = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let Some(device) =
                app_links::approve(&tx, &code, &me.user.id, req.sealed.as_deref(), now)?
            else {
                return Ok(false);
            };
            store::audit(
                &tx,
                now,
                &Event {
                    name: "app_connect",
                    user_id: Some(&me.user.id),
                    device_id: Some(&me.device_id),
                    ip: Some(&ip),
                    detail: Some(&device),
                },
            )?;
            tx.commit()?;
            Ok(true)
        })
        .await?;
    if approved {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(link_invalid())
    }
}

/// `POST /api/v1/auth/app/link/token`, from the app, every few seconds:
/// [`error_code::LINK_PENDING`] until the code is approved, then its
/// session, once. Without the verifier a code says nothing, so no limit is
/// needed for an app asking for ten minutes.
pub async fn link_token(
    State(state): State<AppState>,
    ip: ClientIp,
    Body(req): Body<AppLinkTokenRequest>,
) -> Result<Json<AppToken>, Error> {
    let code = typed(&req.code)?;
    let now = state.clock.now();
    let asked = state
        .db
        .call(move |c| {
            Ok(match app_links::ask(c, &code, &req.verifier, now)? {
                Asked::Approved(redeemed) => {
                    let user = store::user_by_id(c, &redeemed.user_id)?;
                    (Asked::Approved(redeemed), user)
                }
                other => (other, None),
            })
        })
        .await?;
    let (redeemed, user) = match asked {
        (Asked::Pending, _) => {
            return Err(Error::new(
                StatusCode::BAD_REQUEST,
                error_code::LINK_PENDING,
                "not approved yet, ask again in a few seconds",
            ));
        }
        (Asked::Approved(redeemed), Some(user)) => (redeemed, user),
        _ => return Err(link_invalid()),
    };
    not_disabled(&user)?;
    let session = finish_sign_in(
        &state,
        user,
        redeemed.device,
        &ip.0.to_string(),
        Some("with a code on the server's page".to_owned()),
    )
    .await?;
    Ok(Json(AppToken {
        session,
        sealed: redeemed.sealed,
    }))
}
