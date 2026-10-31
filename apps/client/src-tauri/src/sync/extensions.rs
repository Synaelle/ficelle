// SPDX-License-Identifier: AGPL-3.0-or-later

//! What the instance this device is signed in to says about themes: its
//! rules and the themes it offers. The page keeps the rules.

use std::io::Read;

use app_protocol::extensions::Extensions;
use tauri::{AppHandle, State};

use super::sharing::{authed, blocking, failed};
use super::sign_in::SignInError;
use crate::state::AppState;

/// A theme file, at most.
const MAX_THEME: u64 = 2 * 1024 * 1024;

fn valid_id(id: &str) -> bool {
    (1..=40).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The instance's rules and offers, `None` without an account or with a
/// server from before them.
#[tauri::command]
pub async fn instance_extensions(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<Extensions>, SignInError> {
    let linked = state
        .quietly(|s| s.store.sync_link())
        .map_err(|e| failed("vault", format!("{e:?}")))?
        .is_some_and(|l| l.tokens.is_some());
    if !linked {
        return Ok(None);
    }
    blocking(app, |state| {
        match authed(state, |api, token| {
            api.get("/sync/extensions", Some(token))?.parse()
        }) {
            Ok(all) => Ok(Some(all)),
            // a server from before extensions: no rules, nothing offered
            Err(e) if e.code == "notFound" => Ok(None),
            Err(e) => Err(e),
        }
    })
    .await
}

fn download(state: &AppState, path: &str, max: u64) -> Result<Vec<u8>, SignInError> {
    let mut reader = authed(state, |api, token| api.get_stream(path, token))?;
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| failed("offline", e.to_string()))?;
    if bytes.len() as u64 > max {
        return Err(failed("tooLarge", "larger than the server allows"));
    }
    Ok(bytes)
}

/// A theme the instance offers, as its json text.
#[tauri::command]
pub async fn instance_theme(app: AppHandle, id: String) -> Result<String, SignInError> {
    if !valid_id(&id) {
        return Err(failed("invalid", "not a theme id"));
    }
    blocking(app, move |state| {
        let bytes = download(state, &format!("/sync/extensions/themes/{id}"), MAX_THEME)?;
        String::from_utf8(bytes).map_err(|_| failed("invalid", "the theme is not text"))
    })
    .await
}
