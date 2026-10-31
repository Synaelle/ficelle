// SPDX-License-Identifier: AGPL-3.0-or-later

//! The signed in account: who am I, my password, my devices.

use app_protocol::account::{ChangePasswordRequest, Device, User};
use app_protocol::e2ee::AccountKind;
use app_protocol::error_code;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use super::auth::{check_password, hash_password};
use super::extract::{Body, ClientIp, CurrentUser};
use super::user_json;
use crate::accounts::password::{self, Check};
use crate::accounts::store::{self, Event};
use crate::error::Error;
use crate::routes::AppState;

/// `GET /api/v1/me`
pub async fn me(CurrentUser(me): CurrentUser) -> Json<User> {
    Json(user_json(&me.user))
}

/// `POST /api/v1/me/password`: needs the current password, signs out every
/// other device.
pub async fn change_password(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(req): Body<ChangePasswordRequest>,
) -> Result<StatusCode, Error> {
    if me.user.kind == AccountKind::E2ee {
        return Err(Error::new(
            StatusCode::CONFLICT,
            error_code::E2EE_ACCOUNT,
            "an end-to-end encrypted account changes its password through /me/e2ee/password",
        ));
    }
    let now = state.clock.now();
    // as a sign-in: a stolen session guesses no faster than anyone
    super::auth::rate_limit(&state, ip)?;
    super::auth::not_locked(&me.user, now)?;
    let ip_text = ip.0.to_string();
    if check_password(&state, req.current, Some(me.user.password_hash.clone())).await? == Check::Bad
    {
        let id = me.user.id.clone();
        let auth = state.config.auth.clone();
        state
            .db
            .call(move |c| {
                store::record_failure(c, &id, now, &auth)?;
                store::audit(
                    c,
                    now,
                    &Event {
                        name: "password_change_failed",
                        user_id: Some(&id),
                        ip: Some(&ip_text),
                        ..Event::default()
                    },
                )
            })
            .await?;
        return Err(Error::new(
            StatusCode::UNAUTHORIZED,
            error_code::BAD_CREDENTIALS,
            "the current password is wrong",
        ));
    }
    if let Some(why) = password::weakness(&req.new, &me.user.username) {
        return Err(Error::new(
            StatusCode::BAD_REQUEST,
            error_code::PASSWORD_WEAK,
            why,
        ));
    }
    let hash = hash_password(&state, req.new).await?;
    state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            store::set_password(&tx, &me.user.id, &hash)?;
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
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /api/v1/me/devices`
pub async fn devices(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
) -> Result<Json<Vec<Device>>, Error> {
    let user_id = me.user.id.clone();
    let rows = state
        .db
        .call(move |c| store::list_devices(c, &user_id))
        .await?;
    Ok(Json(
        rows.into_iter()
            .map(|d| Device {
                current: d.id == me.device_id,
                id: d.id,
                name: d.name,
                platform: d.platform,
                created_at: d.created_at,
                last_seen_at: d.last_seen_at,
            })
            .collect(),
    ))
}

/// `DELETE /api/v1/me/devices/{id}`: signs one of my devices out.
pub async fn revoke_device(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Path(device_id): Path<String>,
) -> Result<StatusCode, Error> {
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    let done = state
        .db
        .call(move |c| {
            if !store::owns_device(c, &me.user.id, &device_id)? {
                return Ok(false);
            }
            store::revoke_device(c, &device_id, now)?;
            store::audit(
                c,
                now,
                &Event {
                    name: "device_revoked",
                    user_id: Some(&me.user.id),
                    device_id: Some(&device_id),
                    ip: Some(&ip_text),
                    ..Event::default()
                },
            )?;
            Ok(true)
        })
        .await?;
    if done {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(Error::new(
            StatusCode::NOT_FOUND,
            error_code::NOT_FOUND,
            "no such device",
        ))
    }
}
