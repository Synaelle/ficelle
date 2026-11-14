// SPDX-License-Identifier: AGPL-3.0-or-later

//! An app signing in through the server's page, see [`crate::app_codes`]:
//! the page asks for a code once the person agreed, the app trades it for
//! its session.

use app_protocol::account::{AppCode, AppCodeRequest, AppToken, AppTokenRequest};
use axum::Json;
use axum::extract::State;

use super::auth::{finish_sign_in, not_disabled, rate_limit};
use super::extract::{Body, ClientIp, CurrentUser};
use crate::accounts::store::{self, Event};
use crate::app_codes;
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
