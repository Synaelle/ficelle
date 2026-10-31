// SPDX-License-Identifier: AGPL-3.0-or-later

//! The live connection: a WebSocket to the server that says when another
//! device pushed, so this one pulls at once instead of at the next minute.
//! It carries no notes, only notices, and the sync thread does the work.
//! Down (no network, server gone, an old server without it), sync goes on
//! as before and the socket is tried again after a backoff.

use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use app_core::sync::engine::{Backoff, RemoteError};
use app_protocol::sync::{LiveMessage, LiveRequest};
use app_protocol::{PROTOCOL_HEADER, PROTOCOL_VERSION};
use tauri::{AppHandle, Emitter, Manager};
use tungstenite::client::IntoClientRequest;
use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::{Message, WebSocket};

use super::http::Api;
use super::{Sync, current_tokens, emit_status, random, refresh};
use crate::state::AppState;

/// Not linked, locked or signed out: look again after this.
const IDLE: Duration = Duration::from_secs(3);
/// Reads wake this often, to say a newly opened note.
const WAKE: Duration = Duration::from_millis(500);
/// And this often to see whether the link is still the same.
const LOOK: Duration = Duration::from_secs(15);
/// The server pings every 30 seconds: this long without a word, the
/// connection is gone even if no error says so (a laptop that slept).
const SILENT: Duration = Duration::from_secs(75);

/// The thread: connect while linked, reconnect after a backoff.
pub fn run(sync: &Sync, app: &AppHandle) {
    // a reconnect costs one request and is what brings the device back:
    // tried at least every minute
    let mut backoff = Backoff::capped(Duration::from_mins(1));
    loop {
        let state = app.state::<AppState>();
        let link = state
            .quietly(|s| s.store.sync_link())
            .ok()
            .flatten()
            .filter(|l| l.tokens.is_some());
        let Some(link) = link else {
            std::thread::sleep(IDLE);
            continue;
        };
        let ended = connection(sync, app, &state, &link.server, &link.account_id);
        sync.set_live(false);
        emit_status(app);
        match ended {
            Ok(()) => backoff.succeeded(),
            Err(_) => std::thread::sleep(backoff.failed(random())),
        }
    }
}

fn ws_url(server: &str) -> String {
    let base = server
        .strip_prefix("https://")
        .map(|rest| format!("wss://{rest}"))
        .or_else(|| {
            server
                .strip_prefix("http://")
                .map(|rest| format!("ws://{rest}"))
        })
        .unwrap_or_else(|| server.to_owned());
    format!("{base}/api/v1/sync/live")
}

/// One connection, until it drops (`Err`) or the link changed (`Ok`).
fn connection(
    sync: &Sync,
    app: &AppHandle,
    state: &AppState,
    server: &str,
    account: &str,
) -> Result<(), String> {
    let api = Api::new(server);
    let tokens = current_tokens(state).map_err(|e| e.to_string())?;
    let mut request = ws_url(server)
        .into_client_request()
        .map_err(|e| e.to_string())?;
    let headers = request.headers_mut();
    headers.insert(
        "authorization",
        format!("Bearer {}", tokens.access)
            .parse()
            .map_err(|_| "token")?,
    );
    headers.insert(
        PROTOCOL_HEADER,
        PROTOCOL_VERSION.to_string().parse().map_err(|_| "header")?,
    );
    let uri = request.uri().clone();
    let host = uri.host().ok_or("no host")?.trim_matches(['[', ']']);
    let port = uri
        .port_u16()
        .unwrap_or(if uri.scheme_str() == Some("wss") {
            443
        } else {
            80
        });
    let address = (host, port)
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("no address")?;
    let stream =
        TcpStream::connect_timeout(&address, Duration::from_secs(10)).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(WAKE))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;
    let (mut socket, _) = match tungstenite::client_tls(request, stream) {
        Ok(connected) => connected,
        // the token ran out: a refresh, then the next try uses the new one
        Err(tungstenite::HandshakeError::Failure(tungstenite::Error::Http(answer)))
            if answer.status() == 401 =>
        {
            return match refresh(state, &api, &tokens.access) {
                Err(RemoteError::SignedOut) => {
                    sync.now();
                    Ok(())
                }
                Ok(_) => Ok(()),
                Err(e) => Err(e.to_string()),
            };
        }
        Err(e) => return Err(e.to_string()),
    };
    sync.set_live(true);
    emit_status(app);
    let mut heard = Instant::now();
    let mut looked = Instant::now();
    // what the server was told on this connection, nothing yet
    let mut said = Said::Nothing;
    loop {
        say_viewing(sync, &mut socket, &mut said)?;
        match socket.read() {
            Ok(message) => {
                heard = Instant::now();
                if let Some(end) = take(sync, app, state, &api, &tokens.access, message) {
                    return end;
                }
            }
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                if looked.elapsed() < LOOK {
                    continue;
                }
                looked = Instant::now();
                let same = state
                    .quietly(|s| s.store.sync_link())
                    .ok()
                    .flatten()
                    .is_some_and(|l| {
                        l.server == server && l.account_id == account && l.tokens.is_some()
                    });
                if !same {
                    close(&mut socket);
                    return Ok(());
                }
                if heard.elapsed() > SILENT {
                    return Err("the server went silent".into());
                }
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// Acts on a message. `Some` ends the connection.
fn take(
    sync: &Sync,
    app: &AppHandle,
    state: &AppState,
    api: &Api,
    access: &str,
    message: Message,
) -> Option<Result<(), String>> {
    match message {
        Message::Text(text) => match serde_json::from_str::<LiveMessage>(&text) {
            // changes may have come while disconnected: pull now
            Ok(LiveMessage::Hello { coediting, .. }) => {
                sync.set_coediting(coediting);
                sync.now();
                emit_status(app);
            }
            Ok(LiveMessage::Changed { .. }) => sync.now(),
            Ok(LiveMessage::Settings { coediting }) => {
                sync.set_coediting(coediting);
                emit_status(app);
            }
            Ok(LiveMessage::Presence { space, viewers }) => {
                sync.set_presence(
                    space.clone(),
                    viewers.iter().map(|v| v.note.clone()).collect(),
                );
                let _ = app.emit(
                    "sync-presence",
                    serde_json::json!({ "space": space, "viewers": viewers }),
                );
            }
            // a newer server's message this app does not know
            Err(_) => {}
        },
        Message::Close(frame) => {
            // 4401: the token ran out or the device was signed out. A
            // refresh tells which, and gives the next connection its token
            if frame.is_some_and(|f| f.code == CloseCode::from(4401)) {
                match refresh(state, api, access) {
                    Err(RemoteError::SignedOut) => {
                        sync.now();
                        return Some(Ok(()));
                    }
                    Ok(_) => return Some(Ok(())),
                    Err(e) => return Some(Err(e.to_string())),
                }
            }
            return Some(Err("closed by the server".into()));
        }
        _ => {}
    }
    None
}

/// Tells the server the note open in the ui, when it changed since it was
/// last told on this connection.
fn say_viewing(
    sync: &Sync,
    socket: &mut WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>,
    said: &mut Said,
) -> Result<(), String> {
    let now = sync.viewing();
    if matches!(said, Said::This(told) if *told == now) {
        return Ok(());
    }
    let (space, note) = now.0.clone().unzip();
    let request =
        serde_json::to_string(&LiveRequest::Viewing { space, note }).map_err(|e| e.to_string())?;
    socket
        .send(Message::Text(request.into()))
        .map_err(|e| e.to_string())?;
    *said = Said::This(now);
    Ok(())
}

/// What a connection told the server the ui views.
enum Said {
    Nothing,
    This((Option<(String, String)>, bool)),
}

fn close(socket: &mut WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>) {
    let _ = socket.close(None);
    let _ = socket.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_addresses_follow_the_server_address() {
        assert_eq!(
            ws_url("https://notes.example.org"),
            "wss://notes.example.org/api/v1/sync/live"
        );
        assert_eq!(
            ws_url("http://192.168.1.20:8080"),
            "ws://192.168.1.20:8080/api/v1/sync/live"
        );
        assert_eq!(
            ws_url("https://example.org/notes"),
            "wss://example.org/notes/api/v1/sync/live"
        );
    }
}
