// SPDX-License-Identifier: AGPL-3.0-or-later

//! The official catalog on this device (see `app_core::catalog`). Signed in,
//! through the server, which offers it only when its admin turned it on.
//! Without an account, straight from the catalog's address, only when the
//! person turned it on in the settings. Either way every entry and file is
//! checked here against the key built into the app.

use std::io::Read;
use std::time::Duration;

use app_core::catalog::{Catalog, CatalogKind};
use app_protocol::catalog::CatalogView;
use serde::Serialize;
use tauri::AppHandle;

use super::sharing::{authed, blocking, failed};
use super::sign_in::SignInError;
use crate::state::AppState;

const MAX_INDEX: u64 = 5 * 1024 * 1024;
const MAX_THEME: u64 = 2 * 1024 * 1024;

/// The catalog's address and key. Dev builds may point at a test catalog,
/// as they open a test vault: release builds always use the brand's.
fn source() -> (String, String) {
    let dev = |name: &str| {
        cfg!(debug_assertions)
            .then(|| std::env::var(name).ok())
            .flatten()
    };
    (
        dev("APP_CATALOG_URL").unwrap_or_else(|| app_brand::CATALOG_URL.to_owned()),
        dev("APP_CATALOG_KEY").unwrap_or_else(|| app_brand::CATALOG_KEY.to_owned()),
    )
}

/// Where the catalog comes from on this device, and what it holds.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogState {
    /// This build has a catalog key: without one there is no catalog.
    available: bool,
    /// `server` (signed in), `direct` (the person's setting) or `none`.
    via: &'static str,
    /// Signed in: the admin turned it on. Direct: always.
    enabled: bool,
    /// The entries that check out.
    catalog: Option<Catalog>,
    /// Why it could not be read.
    error: Option<String>,
    /// The catalog's host, named to the person before they turn it on.
    host: String,
}

fn linked(state: &AppState) -> Result<bool, SignInError> {
    Ok(state
        .quietly(|s| s.store.sync_link())
        .map_err(|e| failed("vault", format!("{e:?}")))?
        .is_some_and(|l| l.tokens.is_some()))
}

fn direct_get(path: &str, max: u64) -> Result<Vec<u8>, SignInError> {
    let (url, _) = source();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        // the name alone: an outside host has no need of the version
        .user_agent(app_brand::SLUG)
        .build()
        .into();
    let response = agent
        .get(format!("{url}{path}"))
        .call()
        .map_err(|e| failed("offline", e.to_string()))?;
    let mut bytes = Vec::new();
    response
        .into_body()
        .into_reader()
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| failed("offline", e.to_string()))?;
    if bytes.len() as u64 > max {
        return Err(failed("tooLarge", "the catalog sent more than it may"));
    }
    Ok(bytes)
}

fn server_get(state: &AppState, path: &str, max: u64) -> Result<Vec<u8>, SignInError> {
    let mut reader = authed(state, |api, token| api.get_stream(path, token))?;
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| failed("offline", e.to_string()))?;
    if bytes.len() as u64 > max {
        return Err(failed("tooLarge", "larger than allowed"));
    }
    Ok(bytes)
}

/// The catalog as this device may see it.
fn read(state: &AppState, direct: bool) -> Result<CatalogState, SignInError> {
    let (url, key) = source();
    let host = url
        .split_once("://")
        .map_or(url.as_str(), |(_, rest)| rest)
        .split('/')
        .next()
        .unwrap_or_default()
        .to_owned();
    let mut out = CatalogState {
        available: !key.is_empty(),
        via: "none",
        enabled: false,
        catalog: None,
        error: None,
        host,
    };
    if !out.available {
        return Ok(out);
    }
    let found = if linked(state)? {
        out.via = "server";
        let view: CatalogView = match authed(state, |api, token| {
            api.get("/sync/catalog", Some(token))?.parse()
        }) {
            Ok(view) => view,
            // a server from before the catalog offers none
            Err(e) if e.code == "notFound" => return Ok(out),
            Err(e) => return Err(e),
        };
        out.enabled = view.enabled;
        out.error = view.error;
        view.catalog
    } else if direct {
        out.via = "direct";
        out.enabled = true;
        match direct_get("catalog.json", MAX_INDEX).and_then(|bytes| {
            serde_json::from_slice::<Catalog>(&bytes)
                .map_err(|e| failed("invalid", format!("catalog.json: {e}")))
        }) {
            Ok(catalog) => Some(catalog),
            Err(e) => {
                out.error = Some(e.message);
                None
            }
        }
    } else {
        None
    };
    if let Some(catalog) = found {
        match app_core::catalog::checked(catalog, &key) {
            Ok(mut catalog) => {
                // the app takes only the catalog's themes
                catalog.entries.retain(|e| e.kind == CatalogKind::Theme);
                out.catalog = Some(catalog);
            }
            Err(e) => out.error = Some(e.to_string()),
        }
    }
    Ok(out)
}

/// What the catalog holds for this device. `direct`: the person turned the
/// catalog on, for a device without an account.
#[tauri::command]
pub async fn catalog_list(app: AppHandle, direct: bool) -> Result<CatalogState, SignInError> {
    blocking(app, move |state| read(state, direct)).await
}

/// The theme `id` in the catalog as read now, and its file, both checked.
fn fetch_theme(state: &AppState, direct: bool, id: &str) -> Result<Vec<u8>, SignInError> {
    let listed = read(state, direct)?;
    let entry = listed
        .catalog
        .and_then(|c| {
            c.entries
                .into_iter()
                .find(|e| e.kind == CatalogKind::Theme && e.id == id)
        })
        .ok_or_else(|| failed("notFound", "not in the catalog"))?;
    let bytes = match listed.via {
        "server" => server_get(state, &format!("/sync/catalog/themes/{id}"), MAX_THEME)?,
        "direct" => direct_get(&entry.file, MAX_THEME)?,
        _ => return Err(failed("notFound", "no catalog here")),
    };
    let (_, key) = source();
    app_core::catalog::check_file(&entry, &bytes, &key)
        .map_err(|e| failed("invalid", e.to_string()))?;
    Ok(bytes)
}

fn valid_id(id: &str) -> bool {
    (1..=40).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A catalog theme, checked, as its json text.
#[tauri::command]
pub async fn catalog_theme(
    app: AppHandle,
    id: String,
    direct: bool,
) -> Result<String, SignInError> {
    if !valid_id(&id) {
        return Err(failed("invalid", "not a theme id"));
    }
    blocking(app, move |state| {
        let bytes = fetch_theme(state, direct, &id)?;
        String::from_utf8(bytes).map_err(|_| failed("invalid", "the theme is not text"))
    })
    .await
}
