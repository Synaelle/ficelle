// SPDX-License-Identifier: AGPL-3.0-or-later

//! The web version at `/web`: the notes app in a browser, its notes kept in
//! that browser and none of them here. Its files are the web build, in the
//! `web` folder of `server.web_dir`. Off until an admin turns it on, and off
//! every path under `/web` answers 404 like an unknown one. Open to people
//! signed in to this server, `/web/` shows the admin page's sign-in until
//! the browser holds a session cookie, and every other file answers 404
//! without one: nothing of the app loads before the sign-in.

use std::path::{Path, PathBuf};

use axum::Router;
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, header};
use axum::response::{Redirect, Response};
use axum::routing::get;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::accounts::token::{self, Kind};
use crate::clock::DAY;
use crate::db::DbError;
use crate::error::Error;
use crate::routes::AppState;

/// The session cookie's name.
pub const COOKIE: &str = "web_session";

/// Who may open the web version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Access {
    /// Not served, the default.
    Off,
    /// People signed in to this server.
    SignedIn,
    /// Anyone with the address.
    Anyone,
}

impl Access {
    fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::SignedIn => "signed_in",
            Self::Anyone => "anyone",
        }
    }
}

/// Who may open the web version now.
///
/// # Errors
///
/// Database errors only.
pub fn access(conn: &Connection) -> Result<Access, DbError> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'web_version'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(match value.as_deref() {
        Some("signed_in") => Access::SignedIn,
        Some("anyone") => Access::Anyone,
        _ => Access::Off,
    })
}

/// Sets who may open the web version. Turning it off or opening it to
/// anyone ends its sessions.
///
/// # Errors
///
/// Database errors only.
pub fn set_access(conn: &Connection, access: Access) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('web_version', ?1)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        [access.as_str()],
    )?;
    if access != Access::SignedIn {
        conn.execute("DELETE FROM web_sessions", [])?;
    }
    Ok(())
}

/// A session for the browser of a device just signed in, its token for the
/// cookie. It lasts `days`, or until the device is signed out.
///
/// # Errors
///
/// Database errors only.
pub fn start_session(
    conn: &Connection,
    device_id: &str,
    now: i64,
    days: u32,
) -> Result<String, DbError> {
    let (token, hash) = token::new_token(Kind::Web);
    conn.execute(
        "INSERT INTO web_sessions (hash, device_id, expires_at) VALUES (?1, ?2, ?3)",
        params![hash.as_slice(), device_id, now + i64::from(days) * DAY],
    )?;
    Ok(token)
}

/// Whether a cookie's token is a live session of an account that may sign in.
///
/// # Errors
///
/// Database errors only.
pub fn session_valid(conn: &Connection, token: &str, now: i64) -> Result<bool, DbError> {
    let hash = token::hash(token);
    Ok(conn
        .query_row(
            "SELECT 1 FROM web_sessions s
             JOIN devices d ON d.id = s.device_id
             JOIN users u ON u.id = d.user_id
             WHERE s.hash = ?1 AND s.expires_at > ?2
               AND d.revoked_at IS NULL AND u.disabled_at IS NULL",
            params![hash.as_slice(), now],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// Removes ended sessions. Returns how many.
///
/// # Errors
///
/// Database errors only.
pub fn cleanup(conn: &Connection, now: i64) -> Result<usize, DbError> {
    Ok(conn.execute("DELETE FROM web_sessions WHERE expires_at <= ?1", [now])?)
}

/// The web build's folder, when the server has one with a page in it.
pub fn folder(state: &AppState) -> Option<PathBuf> {
    let dir = state.config.server.web_dir.as_deref()?.join("web");
    dir.join("index.html").is_file().then_some(dir)
}

/// The cookie that keeps a browser signed in to the web version.
pub fn cookie(state: &AppState, token: &str) -> String {
    let max_age = i64::from(state.config.auth.refresh_token_days) * DAY / 1000;
    let secure = if state.config.hsts() { "; Secure" } else { "" };
    format!(
        "{COOKIE}={token}; Path={}/web/; Max-Age={max_age}; HttpOnly; SameSite=Strict{secure}",
        state.config.base_path()
    )
}

/// Web version routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/web", get(to_folder))
        .route("/web/", get(index))
        .route("/web/{*path}", get(file))
}

enum Gate {
    Closed,
    SignIn,
    Open(PathBuf),
}

async fn gate(state: &AppState, headers: &HeaderMap) -> Result<Gate, Error> {
    let Some(dir) = folder(state) else {
        return Ok(Gate::Closed);
    };
    let token = session_token(headers);
    let now = state.clock.now();
    let (access, signed_in) = state
        .db
        .call(move |c| {
            let access = access(c)?;
            let signed_in = match (&token, access) {
                (Some(token), Access::SignedIn) => session_valid(c, token, now)?,
                _ => false,
            };
            Ok((access, signed_in))
        })
        .await?;
    Ok(match access {
        Access::Off => Gate::Closed,
        Access::Anyone => Gate::Open(dir),
        Access::SignedIn if signed_in => Gate::Open(dir),
        Access::SignedIn => Gate::SignIn,
    })
}

fn session_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|pair| {
            pair.trim()
                .strip_prefix(&format!("{COOKIE}="))
                .map(str::to_owned)
        })
}

async fn to_folder(State(state): State<AppState>, headers: HeaderMap) -> Result<Redirect, Error> {
    match gate(&state, &headers).await? {
        Gate::Closed => Err(Error::not_found()),
        _ => Ok(Redirect::temporary(&format!(
            "{}/web/",
            state.config.base_path()
        ))),
    }
}

async fn index(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, Error> {
    match gate(&state, &headers).await? {
        Gate::Closed => Err(Error::not_found()),
        Gate::SignIn => crate::web::admin_page(State(state)).await,
        Gate::Open(dir) => serve(&dir, "index.html").await,
    }
}

async fn file(
    State(state): State<AppState>,
    UrlPath(path): UrlPath<String>,
    headers: HeaderMap,
) -> Result<Response, Error> {
    match gate(&state, &headers).await? {
        Gate::Open(dir) if plain(&path) => serve(&dir, &path).await,
        _ => Err(Error::not_found()),
    }
}

/// A path the build could have made: a few plain names, none starting with
/// a dot, so nothing can leave the folder.
fn plain(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    parts.len() <= 3
        && parts.iter().all(|p| {
            !p.is_empty()
                && !p.starts_with('.')
                && p.len() <= 96
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '@'))
        })
}

async fn serve(dir: &Path, path: &str) -> Result<Response, Error> {
    let bytes = tokio::fs::read(dir.join(path))
        .await
        .map_err(|_| Error::not_found())?;
    let file = path.rsplit('/').next().unwrap_or(path);
    // built names carry a content hash, fonts change only with a new build.
    // Private: behind the sign-in, no shared cache may hand them out
    let cache = if path.starts_with("assets/") {
        "private, max-age=31536000, immutable"
    } else if path.starts_with("fonts/") {
        "private, max-age=604800"
    } else {
        "no-cache"
    };
    Ok(crate::web::respond(
        bytes,
        crate::web::content_type(file),
        cache,
    ))
}
