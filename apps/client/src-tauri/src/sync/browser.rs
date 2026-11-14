// SPDX-License-Identifier: AGPL-3.0-or-later

//! Signing in through the server's own page in the system browser (OAuth
//! 2.0 for native apps, RFC 8252, with PKCE). The browser has what this
//! app's webview lacks, passkeys and single sign-on above all, and the
//! person signs in where the server is, not in a form any app could draw.
//!
//! The app opens `<server>/connect` with a PKCE challenge, a state, its
//! device name, a one-time public key and where to answer: a listener on
//! this computer's loopback for this sign-in only, or the app's own scheme
//! on Android. The page answers a code, the app trades it with the verifier
//! for its session. An encrypted account's export key comes sealed to the
//! one-time key, the server cannot open it. One sign-in waits at a time, in
//! memory only.

use std::sync::Mutex;

use app_e2ee::Handoff;
use app_protocol::account::{AppToken, AppTokenRequest};
use app_protocol::e2ee::AccountKind;
use serde::Deserialize;
use tauri::{AppHandle, Manager, State};

use super::SyncStatus;
use super::commands::{blocking, can_sign_in, invalid, link};
use super::http::{Api, normalize};
use super::sign_in::{self, SignInError};
use crate::state::AppState;

/// The sign-in waiting for the browser.
#[derive(Default)]
pub struct PendingBrowser(Mutex<Option<Waiting>>);

pub struct Waiting {
    api: Api,
    redirect: String,
    handoff: Handoff,
}

impl PendingBrowser {
    fn slot(&self) -> std::sync::MutexGuard<'_, Option<Waiting>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The waiting sign-in, taken if the answer carries its state. Another
    /// state leaves it waiting: anything on this computer can knock on the
    /// loopback.
    fn take(&self, state: &str) -> Option<Waiting> {
        let mut slot = self.slot();
        if slot.as_ref().is_some_and(|w| w.handoff.state == state) {
            slot.take()
        } else {
            None
        }
    }

    #[cfg(not(target_os = "android"))]
    fn waits_for(&self, state: &str) -> bool {
        self.slot()
            .as_ref()
            .is_some_and(|w| w.handoff.state == state)
    }
}

/// What the browser tab shows once the app has the answer, in the app's
/// language.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(
    target_os = "android",
    expect(dead_code, reason = "the phone's page shows its own words")
)]
pub struct BrowserPage {
    done: String,
    failed: String,
}

/// How a sign-in through the browser ended, for the settings.
#[cfg(not(target_os = "android"))]
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Ended {
    status: Option<SyncStatus>,
    error: Option<SignInError>,
}

/// Opens the server's sign-in page. Returns its address, for the page to
/// open on Android and to offer as a link when no browser came up.
#[tauri::command]
pub async fn sync_browser_start(
    app: AppHandle,
    state: State<'_, AppState>,
    pending: State<'_, PendingBrowser>,
    address: String,
    page: BrowserPage,
) -> Result<String, SignInError> {
    can_sign_in(&state)?;
    let server = normalize(&address).map_err(|why| invalid(why, "not a server address"))?;
    let handoff = Handoff::new();
    #[cfg(not(target_os = "android"))]
    let (redirect, listener) = loopback::bind()?;
    // this build's own scheme, which the activity's intent filter takes
    #[cfg(target_os = "android")]
    let redirect = format!("{}:/connect", app.config().identifier);
    let device = sign_in::device();
    let url = format!(
        "{server}/connect#{}",
        [
            ("challenge", handoff.challenge.as_str()),
            ("state", &handoff.state),
            ("redirect", &redirect),
            ("device", &device.name),
            ("platform", &device.platform),
            ("key", &handoff.public_key()),
        ]
        .iter()
        .map(|(name, value)| format!("{name}={}", escape(value)))
        .collect::<Vec<_>>()
        .join("&")
    );
    #[cfg(not(target_os = "android"))]
    {
        let app = app.clone();
        let state = handoff.state.clone();
        std::thread::spawn(move || loopback::listen(&app, &listener, &state, &page));
    }
    // on a phone the browser comes back to the app, which shows the result
    #[cfg(target_os = "android")]
    drop(page);
    // a newer sign-in takes the place of one still waiting
    *pending.slot() = Some(Waiting {
        api: Api::new(&server),
        redirect,
        handoff,
    });
    // without a browser coming up the settings still offer the link
    #[cfg(not(target_os = "android"))]
    if let Err(err) = open_in_browser(&url) {
        eprintln!("opening the browser: {err}");
    }
    Ok(url)
}

/// Android: the page sent the browser to the app's scheme, the activity
/// handed the address on.
#[tauri::command]
pub async fn sync_browser_finish(app: AppHandle, url: String) -> Result<SyncStatus, SignInError> {
    let (redirect, query) = url
        .split_once('?')
        .ok_or_else(|| invalid("expired", "sign in again"))?;
    let expected = {
        let pending = app.state::<PendingBrowser>();
        pending.slot().as_ref().map(|w| w.redirect.clone())
    };
    if expected.as_deref() != Some(redirect) {
        return Err(invalid("expired", "sign in again"));
    }
    let query = query.to_owned();
    blocking(move || complete(&app, &query)).await
}

/// Stops waiting, the settings closed the sign-in.
#[tauri::command]
pub async fn sync_browser_cancel(pending: State<'_, PendingBrowser>) -> Result<(), SignInError> {
    pending.slot().take();
    Ok(())
}

/// Trades the page's answer for the session and links this vault.
fn complete(app: &AppHandle, query: &str) -> Result<SyncStatus, SignInError> {
    let found = |name: &str| {
        query
            .split('&')
            .find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
            .unwrap_or_default()
    };
    let waiting = app
        .state::<PendingBrowser>()
        .take(found("state"))
        .ok_or_else(|| invalid("expired", "sign in again"))?;
    if !found("error").is_empty() {
        return Err(invalid(
            "cancelled",
            "the sign-in was cancelled on the server's page",
        ));
    }
    let token: AppToken = waiting
        .api
        .post(
            "/auth/app/token",
            None,
            &AppTokenRequest {
                code: found("code").to_owned(),
                verifier: waiting.handoff.verifier.to_string(),
                redirect: waiting.redirect.clone(),
            },
        )?
        .parse()?;
    let state = app.state::<AppState>();
    let server = waiting.api.base().to_owned();
    // signed in to it as encrypted before: the server's word that it is
    // standard now is refused
    let seen = state
        .with(|s| s.store.e2ee_seen(&server, &token.session.user.username))
        .unwrap_or(false);
    if seen && token.session.user.kind == AccountKind::Standard {
        return Err(sign_in::downgraded());
    }
    let export_key = token
        .sealed
        .as_deref()
        .map(|sealed| waiting.handoff.open(sealed))
        .transpose()
        .map_err(|e| SignInError::keys(&e))?;
    let signed = sign_in::finish(
        &waiting.api,
        token.session,
        export_key.as_deref().map(Vec::as_slice),
        None,
    )?;
    link(app, &state, &server, signed)
}

/// Unreserved characters as they are, everything else as `%XX`.
fn escape(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
                char::from(b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// Opens an address in the system's browser.
#[cfg(not(target_os = "android"))]
pub fn open_in_browser(url: &str) -> std::io::Result<()> {
    let opener = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener)
        .arg(url)
        .spawn()
        .map(drop)
}

#[cfg(not(target_os = "android"))]
mod loopback {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::time::{Duration, Instant};

    use tauri::{AppHandle, Emitter, Manager};

    use super::{BrowserPage, Ended, PendingBrowser, SignInError, complete};

    /// How long the page has to answer: time to find a password or a key.
    const WAIT: Duration = Duration::from_mins(10);

    /// Takes requests on the loopback until the page's answer for `state`
    /// comes, the wait ends, or a newer sign-in takes its place.
    pub fn listen(app: &AppHandle, listener: &TcpListener, state: &str, page: &BrowserPage) {
        let until = Instant::now() + WAIT;
        if listener.set_nonblocking(true).is_err() {
            return;
        }
        let pending = app.state::<PendingBrowser>();
        while Instant::now() < until && pending.waits_for(state) {
            match listener.accept() {
                Ok((stream, _)) => {
                    if let Some(ended) = serve(app, stream, state, page) {
                        let _ = app.emit("sync-browser", ended);
                        if let Some(main) = app.get_webview_window("main") {
                            let _ = main.set_focus();
                        }
                        return;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(200));
                }
                Err(_) => break,
            }
        }
        if pending.take(state).is_some() {
            let _ = app.emit(
                "sync-browser",
                Ended {
                    status: None,
                    error: Some(super::invalid("expired", "sign in again")),
                },
            );
        }
    }

    /// A listener on a free port of this computer's loopback, and the
    /// address the page answers to.
    pub fn bind() -> Result<(String, TcpListener), SignInError> {
        let failed = |e: std::io::Error| super::invalid("browser", &e.to_string());
        let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(failed)?;
        let port = listener.local_addr().map_err(failed)?.port();
        Ok((format!("http://127.0.0.1:{port}/callback"), listener))
    }

    /// One request: the answer for this sign-in ends it, anything else is
    /// turned away.
    fn serve(
        app: &AppHandle,
        mut stream: TcpStream,
        state: &str,
        page: &BrowserPage,
    ) -> Option<Ended> {
        let line = request_line(&stream)?;
        let query = line
            .strip_prefix("GET /callback?")
            .and_then(|rest| rest.split_once(' '))
            .map(|(query, _)| query)
            .filter(|query| {
                query
                    .split('&')
                    .any(|p| p.strip_prefix("state=") == Some(state))
            });
        let Some(query) = query else {
            let _ = stream.write_all(
                b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
            );
            return None;
        };
        let ended = match complete(app, query) {
            Ok(status) => Ended {
                status: Some(status),
                error: None,
            },
            Err(error) => Ended {
                status: None,
                error: Some(error),
            },
        };
        let text = if ended.error.is_none() {
            &page.done
        } else {
            &page.failed
        };
        let body = format!(
            "<!doctype html><meta charset=utf-8><meta name=viewport content=\"width=device-width\"><title>{app}</title><style>body{{font:16px system-ui,sans-serif;display:grid;place-items:center;min-height:90vh;margin:0 16px}}@media(prefers-color-scheme:dark){{body{{background:#1b1b1b;color:#eee}}}}</style><p>{text}</p>",
            app = html(app_brand::DISPLAY_NAME),
            text = html(text),
        );
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\ncache-control: no-store\r\nreferrer-policy: no-referrer\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        Some(ended)
    }

    /// The request line only, at most 4 KB, the rest is not needed.
    fn request_line(stream: &TcpStream) -> Option<String> {
        stream.set_nonblocking(false).ok()?;
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
        let mut line = String::new();
        BufReader::new(stream.take(4096))
            .read_line(&mut line)
            .ok()?;
        Some(line)
    }

    fn html(text: &str) -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }
}
