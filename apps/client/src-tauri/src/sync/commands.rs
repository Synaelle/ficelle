// SPDX-License-Identifier: AGPL-3.0-or-later

//! Sync commands for the settings: check a server, sign in, sign out,
//! status, sync now. The network calls run on blocking threads.

use std::sync::Mutex;

use app_core::store::NewLink;
use app_protocol::ServerInfo;
use app_protocol::account::AuthOptions;
use app_protocol::e2ee::AccountKind;
use app_protocol::security::SecondFactor;
use serde::Serialize;
use tauri::{AppHandle, State};

use super::http::{Api, normalize};
use super::sign_in::{self, Outcome, Pending, SignInError, Signed};
use super::{SyncStatus, emit_status, tokens_of};
use crate::state::AppState;

/// A sign-in waiting for its second factor.
#[derive(Default)]
pub struct PendingSignIn(Mutex<Option<Pending>>);

/// What a server address leads to.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerCheck {
    /// The address as it will be used.
    address: String,
    name: String,
    version: String,
    /// The server speaks this app's protocol.
    compatible: bool,
    options: Option<AuthOptions>,
    /// The protocol versions it speaks, shown in About.
    min_protocol: u32,
    max_protocol: u32,
}

/// How a sign-in went.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignInAnswer {
    /// Signed in and linked.
    status: Option<SyncStatus>,
    /// A second factor is needed: which kinds this app can take.
    second_factor: Option<Vec<SecondFactor>>,
}

fn invalid(code: &str, message: &str) -> SignInError {
    SignInError {
        code: code.into(),
        message: message.into(),
        retry_after_seconds: None,
    }
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, SignInError> + Send + 'static,
) -> Result<T, SignInError> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| invalid("internal", &e.to_string()))?
}

#[tauri::command]
pub async fn sync_check_server(address: String) -> Result<ServerCheck, SignInError> {
    let first = normalize(&address).map_err(|why| invalid(why, "not a server address"))?;
    // a home server typed without a scheme often has no certificate: after
    // https, plain http is tried for addresses that may use it, only when
    // nothing answered. A certificate that does not check out may be someone
    // in between, never a reason to go plain;
    // the page asks before signing in to a plain http address
    let fallback = (!address.contains("://"))
        .then(|| normalize(&format!("http://{}", address.trim())).ok())
        .flatten();
    blocking(move || {
        let checked = check(&first);
        match (checked, fallback) {
            (Err(err), Some(plain)) if err.code == "offline" => check(&plain),
            (checked, _) => checked,
        }
    })
    .await
}

fn check(address: &str) -> Result<ServerCheck, SignInError> {
    let api = Api::new(address);
    let info: ServerInfo = api.info()?;
    let compatible =
        (info.min_protocol..=info.max_protocol).contains(&app_protocol::PROTOCOL_VERSION);
    let options = if compatible {
        Some(api.get("/auth/options", None)?.parse()?)
    } else {
        None
    };
    Ok(ServerCheck {
        address: api.base().to_owned(),
        name: info.name,
        version: info.version,
        compatible,
        options,
        min_protocol: info.min_protocol,
        max_protocol: info.max_protocol,
    })
}

fn link(
    app: &AppHandle,
    state: &AppState,
    server: &str,
    signed: Signed,
) -> Result<SyncStatus, SignInError> {
    let session = signed.session;
    let link = NewLink {
        server: server.to_owned(),
        account_id: session.user.id.clone(),
        username: session.user.username.clone(),
        kind: session.user.kind,
        master_key: signed.master_key,
        device_id: session.device_id.clone(),
        tokens: tokens_of(&session),
    };
    state
        .with(|s| {
            s.store.sync_set_link(&link)?;
            // from now on this server's word that it is standard is refused
            if link.kind == AccountKind::E2ee {
                s.store.remember_e2ee(&link.server, &link.username)?;
            }
            Ok(())
        })
        .map_err(|e| invalid("vault", &format!("{e:?}")))?;
    state.sync.reset();
    state.sync.now();
    emit_status(app);
    Ok(state.sync.status(state))
}

fn answer(
    app: &AppHandle,
    state: &AppState,
    pending: &PendingSignIn,
    server: &str,
    outcome: Outcome,
) -> Result<SignInAnswer, SignInError> {
    match outcome {
        Outcome::Done(signed) => Ok(SignInAnswer {
            status: Some(link(app, state, server, signed)?),
            second_factor: None,
        }),
        Outcome::SecondFactor(waiting, methods) => {
            // a passkey needs the browser's WebAuthn, not available here yet
            let methods: Vec<SecondFactor> = methods
                .into_iter()
                .filter(|m| *m != SecondFactor::Passkey)
                .collect();
            if methods.is_empty() {
                return Err(invalid(
                    "passkeyOnly",
                    "this account only has a passkey as second factor",
                ));
            }
            *pending
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(waiting);
            Ok(SignInAnswer {
                status: None,
                second_factor: Some(methods),
            })
        }
    }
}

#[tauri::command]
pub async fn sync_sign_in(
    app: AppHandle,
    state: State<'_, AppState>,
    pending: State<'_, PendingSignIn>,
    address: String,
    username: String,
    password: String,
) -> Result<SignInAnswer, SignInError> {
    if state.session().is_none() {
        return Err(invalid("locked", "unlock the vault first"));
    }
    let address = normalize(&address).map_err(|why| invalid(why, "not a server address"))?;
    let server = address.clone();
    let known_e2ee = state
        .with(|s| s.store.e2ee_seen(&address, &username))
        .unwrap_or(false);
    let outcome = blocking(move || {
        let api = Api::new(&address);
        sign_in::sign_in(&api, &username, &password, known_e2ee)
    })
    .await?;
    answer(&app, &state, &pending, &server, outcome)
}

#[tauri::command]
pub async fn sync_second_factor(
    app: AppHandle,
    state: State<'_, AppState>,
    pending: State<'_, PendingSignIn>,
    method: SecondFactor,
    code: String,
) -> Result<SyncStatus, SignInError> {
    let waiting = pending
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
        .ok_or_else(|| invalid("expired", "sign in again"))?;
    let server = waiting.api.base().to_owned();
    let signed = blocking(move || sign_in::second_factor(&waiting, method, &code)).await?;
    link(&app, &state, &server, signed)
}

/// Sends a code by email for the sign-in waiting for its second step.
#[tauri::command]
pub async fn sync_email_code(pending: State<'_, PendingSignIn>) -> Result<(), SignInError> {
    let (api, challenge) = pending
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .map(|w| (w.api.clone(), w.challenge.clone()))
        .ok_or_else(|| invalid("expired", "sign in again"))?;
    blocking(move || {
        api.post(
            "/auth/second-factor/email",
            None,
            &app_protocol::security::EmailCodeRequest { challenge },
        )
        .map(|_| ())
        .map_err(SignInError::from)
    })
    .await
}

#[tauri::command]
pub async fn sync_status(state: State<'_, AppState>) -> Result<SyncStatus, SignInError> {
    Ok(state.sync.status(&state))
}

/// The note open in the ui, with its shared space: the others in the
/// space see who is in it. Nothing for a note of one's own.
#[tauri::command]
pub async fn sync_viewing(
    state: State<'_, AppState>,
    space: Option<String>,
    note: Option<String>,
) -> Result<(), SignInError> {
    state.sync.set_viewing(space.zip(note));
    Ok(())
}

#[tauri::command]
pub async fn sync_now(state: State<'_, AppState>) -> Result<(), SignInError> {
    state.sync.now();
    Ok(())
}

/// Stops syncing. The server forgets this device, the vault forgets the
/// account and its key, the notes stay here.
#[tauri::command]
pub async fn sync_disconnect(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<SyncStatus, SignInError> {
    let link = state
        .with(|s| s.store.sync_link())
        .map_err(|e| invalid("vault", &format!("{e:?}")))?;
    if let Some(link) = link
        && let Some(tokens) = link.tokens
    {
        // best effort: offline, the device stays listed until it expires
        let _ = blocking(move || {
            let api = Api::new(&link.server);
            api.post("/auth/logout", Some(&tokens.access), &serde_json::json!({}))?;
            Ok(())
        })
        .await;
    }
    state
        .with(|s| s.store.sync_unlink())
        .map_err(|e| invalid("vault", &format!("{e:?}")))?;
    state.sync.reset();
    emit_status(&app);
    Ok(state.sync.status(&state))
}
