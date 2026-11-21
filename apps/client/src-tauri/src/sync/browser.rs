// SPDX-License-Identifier: AGPL-3.0-or-later

//! Signing in through the server's own page in the system browser (OAuth
//! 2.0 for native apps, RFC 8252, with PKCE). The browser has what this
//! app's webview lacks, passkeys and single sign-on above all, and the
//! person signs in where the server is, not in a form any app could draw.
//!
//! Two ways, one waiting at a time, in memory only:
//!
//! - On a computer, the app opens `<server>/connect` with a PKCE challenge,
//!   a state, its device name, a one-time public key and a listener on this
//!   computer's loopback for this sign-in only. The page answers a code
//!   there, the app trades it with the verifier for its session.
//! - With a code, on a phone or for another device's browser: the app asks
//!   the server for a code, the person types it on `<server>/link` (the
//!   phone opens that page with the code filled in) and agrees, and the app
//!   asks every few seconds until the server hands over its session. No
//!   browser has to send anything back to the app, which some phone
//!   browsers will not do.
//!
//! An encrypted account's export key comes sealed to the one-time key, the
//! server cannot open it.

use std::sync::Mutex;
use std::time::Duration;

use app_e2ee::Handoff;
use app_protocol::account::{AppLink, AppLinkRequest, AppLinkTokenRequest, AppToken};
use app_protocol::e2ee::AccountKind;
use app_protocol::error_code;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use super::SyncStatus;
use super::commands::{can_sign_in, invalid, link};
use super::http::{Api, HttpError, normalize};
use super::sign_in::{self, SignInError};
use crate::state::AppState;

/// The sign-in waiting for the browser.
#[derive(Default)]
pub struct PendingBrowser(Mutex<Option<Waiting>>);

pub struct Waiting {
    api: Api,
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

    #[cfg(desktop)]
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
#[cfg_attr(mobile, expect(dead_code, reason = "a phone signs in with a code"))]
pub struct BrowserPage {
    done: String,
    failed: String,
}

/// How a sign-in through the browser ended, for the settings.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Ended {
    status: Option<SyncStatus>,
    error: Option<SignInError>,
}

impl Ended {
    fn of(result: Result<SyncStatus, SignInError>) -> Self {
        match result {
            Ok(status) => Self {
                status: Some(status),
                error: None,
            },
            Err(error) => Self {
                status: None,
                error: Some(error),
            },
        }
    }
}

/// Tells the settings, and brings the app to the front.
fn announce(app: &AppHandle, ended: Ended) {
    let _ = app.emit("sync-browser", ended);
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.set_focus();
    }
}

/// A code to sign in with, for the settings to show.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkStarted {
    /// `ABCD-EFGH`.
    code: String,
    /// Where it is typed, `<server>/link`.
    page: String,
    /// The same page with the code filled in, for this device's browser.
    url: String,
    expires_at: i64,
}

/// Asks the server for a code and waits in the background until it is
/// approved on the server's page. With `offer`, the token of the QR code the
/// phone scanned there: the page then shows this app to approve, no code to
/// type. The end comes as a `sync-browser` event.
#[tauri::command]
pub async fn sync_link_start(
    app: AppHandle,
    state: State<'_, AppState>,
    address: String,
    offer: Option<String>,
) -> Result<LinkStarted, SignInError> {
    can_sign_in(&state)?;
    let server = normalize(&address).map_err(|why| invalid(why, "not a server address"))?;
    tauri::async_runtime::spawn_blocking(move || {
        let handoff = Handoff::new();
        let api = Api::new(&server);
        let given: AppLink = api
            .post(
                "/auth/app/link",
                None,
                &AppLinkRequest {
                    challenge: handoff.challenge.clone(),
                    key: handoff.public_key(),
                    device: sign_in::device(),
                    offer,
                },
            )?
            .parse()?;
        let started = LinkStarted {
            page: format!("{server}/link"),
            // a phone's browser brings back the tab it had for this app when
            // only the # part differs: each sign-in's address is its own
            url: format!(
                "{server}/link?for={}#code={}",
                given.expires_at,
                escape(&given.code)
            ),
            code: given.code.clone(),
            expires_at: given.expires_at,
        };
        let id = handoff.state.clone();
        // a newer sign-in takes the place of one still waiting
        *app.state::<PendingBrowser>().slot() = Some(Waiting { api, handoff });
        let wait = Duration::from_secs(u64::from(given.interval.clamp(1, 30)));
        std::thread::spawn(move || ask_until_approved(&app, &id, &given.code, wait));
        Ok(started)
    })
    .await
    .map_err(|e| invalid("internal", &e.to_string()))?
}

/// Asks the server whether the code was approved, every `wait`, until it
/// is, it expires, or the sign-in is cancelled or replaced. A phone that
/// loses the network for a moment keeps asking.
fn ask_until_approved(app: &AppHandle, id: &str, code: &str, wait: Duration) {
    let pending = app.state::<PendingBrowser>();
    loop {
        std::thread::sleep(wait);
        let Some((api, verifier)) = pending
            .slot()
            .as_ref()
            .filter(|w| w.handoff.state == id)
            .map(|w| (w.api.clone(), w.handoff.verifier.to_string()))
        else {
            return;
        };
        let answer = api.post(
            "/auth/app/link/token",
            None,
            &AppLinkTokenRequest {
                code: code.to_owned(),
                verifier,
            },
        );
        let ended = match answer {
            Err(HttpError::Api { body, .. }) if body.error == error_code::LINK_PENDING => continue,
            Err(HttpError::Offline(_)) => continue,
            Err(HttpError::Api { body, .. }) if body.error == error_code::LINK_INVALID => {
                Err(invalid("expired", "sign in again"))
            }
            Err(err) => Err(SignInError::from(err)),
            Ok(answer) => answer.parse::<AppToken>().map_err(SignInError::from),
        };
        let Some(waiting) = pending.take(id) else {
            return;
        };
        announce(
            app,
            Ended::of(ended.and_then(|token| finish(app, &waiting, token))),
        );
        return;
    }
}

/// Opens the server's sign-in page in the browser, with a listener on the
/// loopback for its answer. Returns its address, to offer as a link when no
/// browser came up.
#[cfg(desktop)]
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
    let (redirect, listener) = loopback::bind()?;
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
    {
        let app = app.clone();
        let state = handoff.state.clone();
        let redirect = redirect.clone();
        std::thread::spawn(move || loopback::listen(&app, &listener, &state, &redirect, &page));
    }
    // a newer sign-in takes the place of one still waiting
    *pending.slot() = Some(Waiting {
        api: Api::new(&server),
        handoff,
    });
    // without a browser coming up the settings still offer the link
    if let Err(err) = open_in_browser(&url) {
        eprintln!("opening the browser: {err}");
    }
    Ok(url)
}

/// Phones sign in with a code, see [`sync_link_start`].
#[cfg(mobile)]
#[tauri::command]
pub async fn sync_browser_start(address: String, page: BrowserPage) -> Result<String, SignInError> {
    drop((address, page));
    Err(invalid("browser", "a phone signs in with a code"))
}

/// Stops waiting, the settings closed the sign-in.
#[tauri::command]
pub async fn sync_browser_cancel(pending: State<'_, PendingBrowser>) -> Result<(), SignInError> {
    pending.slot().take();
    Ok(())
}

/// The session the server handed over, checked, and this vault linked.
fn finish(app: &AppHandle, waiting: &Waiting, token: AppToken) -> Result<SyncStatus, SignInError> {
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
#[cfg(desktop)]
pub fn open_in_browser(url: &str) -> std::io::Result<()> {
    let opener = if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener)
        .arg(url)
        .spawn()
        .map(drop)
}

#[cfg(desktop)]
mod loopback {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::time::{Duration, Instant};

    use app_protocol::account::{AppToken, AppTokenRequest};
    use tauri::{AppHandle, Manager};

    use super::{BrowserPage, Ended, PendingBrowser, SignInError, SyncStatus, announce, finish};

    /// How long the page has to answer: time to find a password or a key.
    const WAIT: Duration = Duration::from_mins(10);

    /// Takes requests on the loopback until the page's answer for `state`
    /// comes, the wait ends, or a newer sign-in takes its place.
    pub fn listen(
        app: &AppHandle,
        listener: &TcpListener,
        state: &str,
        redirect: &str,
        page: &BrowserPage,
    ) {
        let until = Instant::now() + WAIT;
        if listener.set_nonblocking(true).is_err() {
            return;
        }
        let pending = app.state::<PendingBrowser>();
        while Instant::now() < until && pending.waits_for(state) {
            match listener.accept() {
                Ok((stream, _)) => {
                    if let Some(ended) = serve(app, stream, state, redirect, page) {
                        announce(app, ended);
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
            announce(
                app,
                Ended::of(Err(super::invalid("expired", "sign in again"))),
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
        redirect: &str,
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
        let ended = Ended::of(trade(app, query, state, redirect));
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

    /// Trades the page's answer for the session and links this vault.
    fn trade(
        app: &AppHandle,
        query: &str,
        state: &str,
        redirect: &str,
    ) -> Result<SyncStatus, SignInError> {
        let found = |name: &str| {
            query
                .split('&')
                .find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
                .unwrap_or_default()
        };
        let waiting = app
            .state::<PendingBrowser>()
            .take(state)
            .ok_or_else(|| super::invalid("expired", "sign in again"))?;
        if !found("error").is_empty() {
            return Err(super::invalid(
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
                    redirect: redirect.to_owned(),
                },
            )?
            .parse()?;
        finish(app, &waiting, token)
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
