// SPDX-License-Identifier: AGPL-3.0-or-later

//! Calls to the server outside a sync round, with this device's tokens.

use app_core::store::Tokens;
use app_core::sync::engine::RemoteError;
use tauri::{AppHandle, Manager};

use super::http::{Api, HttpError};
use super::sign_in::SignInError;
use crate::error::CommandError;
use crate::state::AppState;

pub(super) fn failed(code: &str, message: impl Into<String>) -> SignInError {
    SignInError {
        code: code.into(),
        message: message.into(),
        retry_after_seconds: None,
    }
}

fn vault_error(err: &CommandError) -> SignInError {
    failed("vault", err.message())
}

/// Runs a call to the server with this device's tokens, refreshed once
/// when they ran out.
pub(super) fn authed<T>(
    state: &AppState,
    call: impl Fn(&Api, &str) -> Result<T, HttpError>,
) -> Result<T, SignInError> {
    let link = state
        .quietly(|s| s.store.sync_link())
        .map_err(|e| vault_error(&e))?
        .ok_or_else(|| failed("signedOut", "not signed in"))?;
    let tokens: Tokens = link
        .tokens
        .ok_or_else(|| failed("signedOut", "sign in again"))?;
    let api = Api::new(&link.server);
    match call(&api, &tokens.access) {
        Err(HttpError::Api { status: 401, .. }) => {
            let fresh = super::refresh(state, &api, &tokens.access).map_err(|e| match e {
                RemoteError::Offline(why) => failed("offline", why),
                RemoteError::SignedOut => failed("signedOut", "sign in again"),
                RemoteError::Refused(why) => failed("refused", why),
            })?;
            Ok(call(&api, &fresh.access)?)
        }
        other => Ok(other?),
    }
}

pub(super) async fn blocking<T: Send + 'static>(
    app: AppHandle,
    f: impl FnOnce(&AppState) -> Result<T, SignInError> + Send + 'static,
) -> Result<T, SignInError> {
    tauri::async_runtime::spawn_blocking(move || f(&app.state::<AppState>()))
        .await
        .map_err(|e| failed("internal", e.to_string()))?
}
