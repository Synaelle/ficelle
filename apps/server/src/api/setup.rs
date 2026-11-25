// SPDX-License-Identifier: AGPL-3.0-or-later

//! `/api/v1/setup`: the first admin, from the link in the log to the lock.
//! Each call is rate limited per address, refused unless the request passes
//! the https rule, and answers 410 once setup is locked, for good. The admin
//! is a standard account or, registered over OPAQUE from the browser, an
//! end-to-end encrypted one. Instead, a fresh server may restore a backup
//! without a password, or one locked by a backup key, which the browser
//! opens with the account's recovery code: the code never reaches the
//! server, the backup key does.

use app_e2ee::server::Setup;
use app_protocol::e2ee::{KeyBundle, OpaqueRegistrationResponse};
use app_protocol::error_code;
use app_protocol::security::TotpSetup;
use app_protocol::setup::{
    SetupChecks, SetupE2ee, SetupFactor, SetupFactorRequest, SetupFinishRequest, SetupFinished,
    SetupRegistrationRequest, SetupStartRequest, SetupStarted, SetupStatus,
};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde_json::Value;
use webauthn_rs::prelude::{PasskeyRegistration, RegisterPublicKeyCredential, Uuid};

use super::auth::{hash_password, new_credentials, rate_limit, session};
use super::extract::{Body, ClientIp, Peer, Secure};
use crate::accounts::store::{self, Event};
use crate::accounts::{device_info, display_name, e2ee, factors, normal_username, totp};
use crate::backups::{self, KeyFile, ServerTarget};
use crate::error::Error;
use crate::routes::AppState;
use crate::setup;

fn gone() -> Error {
    Error::new(
        StatusCode::GONE,
        error_code::SETUP_DONE,
        "setup is done and locked, sign in instead",
    )
}

fn link_invalid() -> Error {
    Error::new(
        StatusCode::FORBIDDEN,
        error_code::SETUP_LINK_INVALID,
        "this setup link or session is wrong, used or expired: restart the server for a new link in its log",
    )
}

/// A session that is not there. A request racing another one that just
/// finished setup finds its session gone with it: setup is done, not the
/// link wrong.
fn no_session(state: &AppState) -> Error {
    if state.setup.is_done() {
        gone()
    } else {
        link_invalid()
    }
}

/// Refuses once setup is locked, asking the database when memory does not
/// know yet: the command line may have made the first admin.
async fn not_locked(state: &AppState) -> Result<(), Error> {
    if state.setup.is_done() {
        return Err(gone());
    }
    if state.setup.is_restored() {
        return Err(Error::new(
            StatusCode::GONE,
            error_code::SETUP_RESTORED,
            "a backup was restored here: restart the server to open it",
        ));
    }
    if state.db.call(|c| setup::is_locked(c)).await? {
        state.setup.set_done();
        return Err(gone());
    }
    Ok(())
}

/// The checks every call but the status makes first.
async fn guard(state: &AppState, ip: ClientIp, secure: Secure) -> Result<(), Error> {
    rate_limit(state, ip)?;
    not_locked(state).await?;
    if !secure.0 {
        return Err(Error::new(
            StatusCode::FORBIDDEN,
            error_code::HTTPS_REQUIRED,
            "open the setup link over https, or from the server itself",
        ));
    }
    Ok(())
}

async fn audit(state: &AppState, name: &'static str, ip: ClientIp, detail: Option<&'static str>) {
    let now = state.clock.now();
    let ip = ip.0.to_string();
    let logged = state
        .db
        .call(move |c| {
            store::audit(
                c,
                now,
                &Event {
                    name,
                    ip: Some(&ip),
                    detail,
                    ..Event::default()
                },
            )
        })
        .await;
    if let Err(err) = logged {
        tracing::warn!("security log: {err}");
    }
}

/// `GET /api/v1/setup`
pub async fn status(
    State(state): State<AppState>,
    secure: Secure,
) -> Result<Json<SetupStatus>, Error> {
    not_locked(&state).await?;
    Ok(Json(SetupStatus {
        secure: secure.0,
        passkeys: state.webauthn.is_some(),
        public_url: state.config.server.public_url.clone(),
    }))
}

/// `POST /api/v1/setup/start`: the link's token for a session.
pub async fn start(
    State(state): State<AppState>,
    ip: ClientIp,
    secure: Secure,
    Body(req): Body<SetupStartRequest>,
) -> Result<Json<SetupStarted>, Error> {
    guard(&state, ip, secure).await?;
    let Some((session, expires_at)) = state.setup.start(&req.token, state.clock.now()) else {
        audit(&state, "setup_failed", ip, Some("setup link")).await;
        return Err(link_invalid());
    };
    audit(&state, "setup_started", ip, None).await;
    tracing::info!("setup link used from {}", ip.0);
    Ok(Json(SetupStarted {
        session,
        expires_at,
    }))
}

/// A setup session named in a body, for the checks.
#[derive(serde::Deserialize)]
pub struct SessionRequest {
    session: String,
}

/// The header the upload probe carries its session in: the body is the
/// probe itself.
const SESSION_HEADER: &str = "x-setup-session";

fn live_session(state: &AppState, session: &str) -> Result<(), Error> {
    state
        .setup
        .with_session(session, state.clock.now(), |_| ())
        .ok_or_else(link_invalid)
}

/// `POST /api/v1/setup/checks`: what the server sees of this request.
pub async fn checks(
    State(state): State<AppState>,
    ip: ClientIp,
    secure: Secure,
    Peer(peer): Peer,
    headers: axum::http::HeaderMap,
    Body(req): Body<SessionRequest>,
) -> Result<Json<SetupChecks>, Error> {
    guard(&state, ip, secure).await?;
    live_session(&state, &req.session)?;
    let config = &state.config;
    let via_proxy = config.server.trusts(peer);
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.split(',').next().unwrap_or("").trim().to_owned())
            .filter(|v| !v.is_empty())
    };
    let forwarded = |name: &str| if via_proxy { header(name) } else { None };
    let host = forwarded("x-forwarded-host")
        .or_else(|| header("host"))
        .unwrap_or_default()
        .to_ascii_lowercase();
    let public_host = url::Url::parse(&config.server.public_url)
        .ok()
        .and_then(|u| {
            let name = u.host_str()?.to_ascii_lowercase();
            Some(match u.port() {
                Some(port) => format!("{name}:{port}"),
                None => name,
            })
        });
    Ok(Json(SetupChecks {
        secure: secure.0,
        via_proxy,
        forwarded_for: forwarded("x-forwarded-for").is_some(),
        forwarded_proto: forwarded("x-forwarded-proto").map(|p| p.to_ascii_lowercase()),
        client_ip: ip.0.to_string(),
        host_matches: public_host.as_deref() == Some(host.as_str()),
        host,
        public_url: config.server.public_url.clone(),
        server_time: state.clock.now(),
        part_bytes: super::files::part_bytes(&state),
        mail: false,
    }))
}

/// `POST /api/v1/setup/upload`: a body as large as the largest upload part,
/// sent by the page through the proxy. A proxy with a lower limit answers
/// 413 itself, before this runs.
pub async fn upload(
    State(state): State<AppState>,
    ip: ClientIp,
    secure: Secure,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Result<Json<Value>, Error> {
    guard(&state, ip, secure).await?;
    let session = headers
        .get(SESSION_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    live_session(&state, session)?;
    Ok(Json(serde_json::json!({ "received": body.len() })))
}

/// `GET /api/v1/setup/socket`: a WebSocket that says hello and closes, to
/// show the proxy passes upgrades. It needs no session, it tells nothing.
pub async fn socket(
    State(state): State<AppState>,
    ip: ClientIp,
    ws: axum::extract::WebSocketUpgrade,
) -> Result<axum::response::Response, Error> {
    rate_limit(&state, ip)?;
    not_locked(&state).await?;
    Ok(ws.on_upgrade(|mut socket| async move {
        use axum::extract::ws::Message;
        let _ = socket.send(Message::Text("ok".into())).await;
        let _ = socket.send(Message::Close(None)).await;
    }))
}

fn username_of(raw: &str) -> Result<String, Error> {
    normal_username(raw).ok_or_else(|| {
        Error::new(
            StatusCode::BAD_REQUEST,
            error_code::USERNAME_INVALID,
            "use 3 to 32 of a-z, 0-9, dot, dash and underscore, starting with a letter or digit",
        )
    })
}

/// `POST /api/v1/setup/totp`: an authenticator app secret for the admin.
pub async fn totp_setup(
    State(state): State<AppState>,
    ip: ClientIp,
    secure: Secure,
    Body(req): Body<SetupFactorRequest>,
) -> Result<Json<TotpSetup>, Error> {
    guard(&state, ip, secure).await?;
    let username = username_of(&req.username)?;
    let secret = totp::new_secret();
    let kept = secret.clone();
    state
        .setup
        .with_session(&req.session, state.clock.now(), |s| s.totp = Some(kept))
        .ok_or_else(link_invalid)?;
    let host = url::Url::parse(&state.config.server.public_url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned));
    let account = host.map_or_else(|| username.clone(), |h| format!("{username}@{h}"));
    let uri = totp::uri(&secret, app_brand::DISPLAY_NAME, &account);
    Ok(Json(TotpSetup {
        qr_svg: totp::qr_svg(&uri),
        secret,
        uri,
    }))
}

/// `POST /api/v1/setup/passkey`: options for `navigator.credentials.create`,
/// for the admin's account id.
pub async fn passkey_setup(
    State(state): State<AppState>,
    ip: ClientIp,
    secure: Secure,
    Body(req): Body<SetupFactorRequest>,
) -> Result<Json<Value>, Error> {
    guard(&state, ip, secure).await?;
    let webauthn = super::passkeys::relying_party(&state)?;
    let username = username_of(&req.username)?;
    let display = display_name(req.display_name.as_deref(), &username);
    let now = state.clock.now();
    let user_id = state
        .setup
        .with_session(&req.session, now, |s| s.user_id.clone())
        .ok_or_else(link_invalid)?;
    let handle = Uuid::parse_str(&user_id).map_err(|e| Error::internal(&e.to_string()))?;
    let (options, registration) = webauthn
        .start_passkey_registration(handle, &username, &display, None)
        .map_err(|e| Error::internal(&e.to_string()))?;
    state
        .setup
        .with_session(&req.session, now, |s| s.passkey = Some(registration))
        .ok_or_else(link_invalid)?;
    let mut options = serde_json::to_value(options).map_err(|e| Error::internal(&e.to_string()))?;
    super::passkeys::discoverable(&mut options);
    Ok(Json(options))
}

/// `POST /api/v1/setup/registration`: the OPAQUE answer for an end-to-end
/// encrypted admin. Keeps nothing, like `/auth/opaque/registration` once
/// setup is done.
pub async fn registration(
    State(state): State<AppState>,
    ip: ClientIp,
    secure: Secure,
    Body(req): Body<SetupRegistrationRequest>,
) -> Result<Json<OpaqueRegistrationResponse>, Error> {
    guard(&state, ip, secure).await?;
    live_session(&state, &req.session)?;
    let username = username_of(&req.username)?;
    let response = super::e2ee::setup(&state)
        .await?
        .registration_response(&req.request, &username)
        .map_err(|e| super::e2ee::malformed(&e))?;
    Ok(Json(OpaqueRegistrationResponse { response }))
}

/// An end-to-end encrypted admin, checked before anything is stored.
struct E2eeAdmin {
    record: Vec<u8>,
    ksf: u8,
    keys: KeyBundle,
    verifier: String,
}

/// The admin's username and, for an end-to-end encrypted one, what makes
/// it so.
fn credentials(
    username: &str,
    password: &str,
    e2ee: Option<SetupE2ee>,
) -> Result<(String, Option<E2eeAdmin>), Error> {
    let Some(made) = e2ee else {
        return Ok((new_credentials(username, password)?, None));
    };
    // the browser stretches and checks it, the server never sees it
    if !password.is_empty() {
        return Err(Error::invalid(
            "an end-to-end encrypted admin's password stays in the browser",
        ));
    }
    super::e2ee::check_bundle(&made.keys)?;
    let admin = E2eeAdmin {
        record: Setup::record(&made.upload).map_err(|e| super::e2ee::malformed(&e))?,
        ksf: super::e2ee::ksf(made.ksf)?,
        keys: made.keys,
        verifier: made.recovery_verifier,
    };
    Ok((username_of(username)?, Some(admin)))
}

/// Makes the new admin end-to-end encrypted when it is: what setup was
/// for the log.
fn keep_e2ee(
    tx: &rusqlite::Connection,
    user_id: &str,
    made: Option<&E2eeAdmin>,
    factor: &str,
) -> Result<String, crate::db::DbError> {
    let Some(made) = made else {
        return Ok(factor.to_owned());
    };
    e2ee::make_e2ee(
        tx,
        user_id,
        &made.record,
        made.ksf,
        &made.keys,
        &made.verifier,
    )?;
    Ok(format!("{factor}, e2ee"))
}

/// The second factor checked against the session. `None` when it does not
/// check out.
enum Proven {
    Totp {
        secret: String,
        step: i64,
    },
    Passkey(Box<webauthn_rs::prelude::Passkey>, String),
    /// No second step, chosen by the admin.
    Nothing,
}

fn prove(
    state: &AppState,
    factor: SetupFactor,
    totp_secret: Option<String>,
    passkey: Option<PasskeyRegistration>,
    now: i64,
) -> Result<Option<Proven>, Error> {
    Ok(match factor {
        SetupFactor::Totp { code } => totp_secret.and_then(|secret| {
            totp::check(&secret, &code, now, None).map(|step| Proven::Totp { secret, step })
        }),
        SetupFactor::Passkey { credential, name } => {
            let webauthn = super::passkeys::relying_party(state)?;
            let answer: Option<RegisterPublicKeyCredential> =
                serde_json::from_value(credential).ok();
            match (answer, passkey) {
                (Some(answer), Some(registration)) => webauthn
                    .finish_passkey_registration(&answer, &registration)
                    .ok()
                    .map(|p| Proven::Passkey(Box::new(p), name)),
                _ => None,
            }
        }
        SetupFactor::None => Some(Proven::Nothing),
    })
}

fn passkey_name(name: &str) -> String {
    let name: String = name.chars().filter(|c| !c.is_control()).collect();
    let name: String = name.trim().chars().take(64).collect();
    if name.is_empty() {
        "Passkey".to_owned()
    } else {
        name
    }
}

/// Stores the admin's second step and its recovery codes, what it was for
/// the log.
fn keep_factor(
    tx: &rusqlite::Connection,
    master: &crate::keys::MasterKey,
    user_id: &str,
    proven: Proven,
    codes: &[crate::accounts::token::TokenHash],
    now: i64,
) -> Result<&'static str, crate::db::DbError> {
    let factor = match proven {
        Proven::Totp { secret, step } => {
            factors::set_totp_pending(tx, master, user_id, &secret)?;
            factors::activate_totp(tx, user_id, step)?;
            "totp"
        }
        Proven::Passkey(passkey, name) => {
            factors::insert_passkey(tx, user_id, &passkey_name(&name), &passkey, now)?;
            "passkey"
        }
        Proven::Nothing => {
            // chosen past a warning: admins then go without one, and there
            // is nothing for recovery codes to stand in for
            factors::set_admins_need_second_step(tx, false)?;
            return Ok("none");
        }
    };
    factors::replace_recovery_codes(tx, user_id, codes)?;
    Ok(factor)
}

/// `POST /api/v1/setup/finish`: the admin, its second factor, the first
/// choices and the lock, all at once.
pub async fn finish(
    State(state): State<AppState>,
    ip: ClientIp,
    secure: Secure,
    Body(req): Body<SetupFinishRequest>,
) -> Result<(StatusCode, Json<SetupFinished>), Error> {
    guard(&state, ip, secure).await?;
    let now = state.clock.now();
    let (user_id, totp_secret, passkey) = state
        .setup
        .with_session(&req.session, now, |s| {
            (s.user_id.clone(), s.totp.clone(), s.passkey.clone())
        })
        .ok_or_else(|| no_session(&state))?;
    let (username, e2ee_made) = credentials(&req.username, &req.password, req.e2ee)?;
    let is_totp = matches!(req.factor, SetupFactor::Totp { .. });
    let no_factor = matches!(req.factor, SetupFactor::None);
    let Some(proven) = prove(&state, req.factor, totp_secret, passkey, now)? else {
        state.setup.failed(&req.session);
        audit(&state, "setup_failed", ip, Some("second factor")).await;
        return Err(if is_totp {
            Error::new(
                StatusCode::BAD_REQUEST,
                error_code::TOTP_INVALID,
                "this code does not match, check the time on the phone",
            )
        } else {
            super::passkeys::invalid()
        });
    };
    let display = display_name(req.display_name.as_deref(), &username);
    let hash = if e2ee_made.is_some() {
        String::new()
    } else {
        hash_password(&state, req.password).await?
    };
    let device = device_info(&req.device);
    let auth = state.config.auth.clone();
    // an e2ee admin's recovery code stands in for the second step too
    let codes = if no_factor || e2ee_made.is_some() {
        Vec::new()
    } else {
        totp::new_recovery_codes()
    };
    let hashes: Vec<_> = codes.iter().map(|(_, h)| *h).collect();
    let ip_text = ip.0.to_string();
    let master = state.master.clone();
    let made = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            // a second request racing this one finds setup locked
            if setup::is_locked(&tx)? {
                return Ok(None);
            }
            let Some(user) = store::insert_user_with_id(
                &tx,
                &user_id,
                &username,
                &display,
                &hash,
                app_protocol::account::Role::Admin,
                now,
            )?
            else {
                return Ok(None);
            };
            let factor = keep_factor(&tx, &master, &user.id, proven, &hashes, now)?;
            let detail = keep_e2ee(&tx, &user.id, e2ee_made.as_ref(), factor)?;
            let user = store::user_by_id(&tx, &user.id)?.unwrap_or(user);
            setup::lock(&tx)?;
            let tokens = store::create_session(&tx, &user.id, &device, now, &auth)?;
            store::audit(
                &tx,
                now,
                &Event {
                    name: "setup_done",
                    user_id: Some(&user.id),
                    device_id: Some(&tokens.device_id),
                    ip: Some(&ip_text),
                    detail: Some(&detail),
                },
            )?;
            tx.commit()?;
            Ok(Some((user, tokens)))
        })
        .await?;
    let (user, tokens) = made.ok_or_else(gone)?;
    state.setup.set_done();
    tracing::warn!(
        "first admin {} made, setup is locked. Back up the data folder's secrets (master.key and the databases) somewhere apart from this machine",
        user.username
    );
    let recovery_codes = codes.into_iter().map(|(code, _)| code).collect();
    Ok((
        StatusCode::CREATED,
        Json(SetupFinished {
            session: session(&user, tokens),
            recovery_codes,
        }),
    ))
}

/// `POST /api/v1/setup/restore/look`: a backup folder on the server.
#[derive(serde::Deserialize)]
pub struct RestoreLook {
    session: String,
    path: String,
}

/// What a backup folder needs to restore.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreLooked {
    /// For backups locked by a backup key, what the recovery code opens it
    /// with. `None` for backups without a password.
    key_file: Option<KeyFile>,
}

fn backup_target(path: String) -> Result<std::sync::Arc<dyn app_repo::Target>, Error> {
    ServerTarget::Folder { path }
        .open()
        .map_err(super::backups::failed)
}

/// `POST /api/v1/setup/restore/look`: whether the folder holds backups
/// this page restores, and what opens them. Backups with a person's
/// password restore from the command line.
pub async fn restore_look(
    State(state): State<AppState>,
    ip: ClientIp,
    secure: Secure,
    Body(req): Body<RestoreLook>,
) -> Result<Json<RestoreLooked>, Error> {
    guard(&state, ip, secure).await?;
    live_session(&state, &req.session)?;
    let target = backup_target(req.path)?;
    let looked = tokio::task::spawn_blocking(move || {
        if !app_repo::Repo::exists(target.as_ref()).map_err(|e| e.to_string())? {
            return Err("there is no backup in this folder".to_owned());
        }
        if let Some(file) = backups::read_key_file(target.as_ref())? {
            return Ok(Some(file));
        }
        match app_repo::Repo::open(target, backups::NO_PASSWORD) {
            Ok(_) => Ok(None),
            Err(app_repo::Error::WrongPassword) => Err(
                "these backups have a password: restore them from the command line, the server stopped"
                    .to_owned(),
            ),
            Err(e) => Err(backups::open_error(e)),
        }
    })
    .await
    .map_err(|e| Error::internal(&e.to_string()))?
    .map_err(super::backups::failed)?;
    Ok(Json(RestoreLooked { key_file: looked }))
}

/// `POST /api/v1/setup/restore`: the backup folder and, for locked
/// backups, the backup key the browser opened.
#[derive(serde::Deserialize)]
pub struct RestoreRequest {
    session: String,
    path: String,
    #[serde(default)]
    key: String,
}

/// The snapshot restored.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Restored {
    time: i64,
    users: u64,
    files: u64,
}

/// `POST /api/v1/setup/restore`: restores the newest backup for the next
/// start, which puts it in place of this fresh instance. Setup ends here.
pub async fn restore(
    State(state): State<AppState>,
    ip: ClientIp,
    secure: Secure,
    Body(req): Body<RestoreRequest>,
) -> Result<Json<Restored>, Error> {
    guard(&state, ip, secure).await?;
    live_session(&state, &req.session)?;
    let target = backup_target(req.path.clone())?;
    let password = zeroize::Zeroizing::new(if req.key.is_empty() {
        backups::NO_PASSWORD.to_owned()
    } else {
        req.key
    });
    let scratch = backups::scratch(&state, "restore");
    let server = state.config.clone();
    let key_apart = !server.storage.master_key_file.as_os_str().is_empty();
    let path = req.path;
    let snapshot = tokio::task::spawn_blocking(move || {
        let file = backups::read_key_file(target.as_ref())?;
        let repo = app_repo::Repo::open(target, &password).map_err(backups::open_error)?;
        let data_dir = &server.storage.data_dir;
        let snapshot = backups::instance::restore_for_start(&repo, &scratch, data_dir, key_apart)?;
        if let Some(file) = file {
            let staged = data_dir.join(backups::instance::RESTORED);
            backups::carry_key(&staged, &server, &path, &password, &file)?;
        }
        Ok::<_, String>(snapshot)
    })
    .await
    .map_err(|e| Error::internal(&e.to_string()))?
    .map_err(super::backups::failed)?;
    state.setup.set_restored();
    tracing::warn!(
        "a backup was restored from the setup page from {}: restart the server to open it",
        ip.0
    );
    let count = |name: &str| snapshot.counts.get(name).copied().unwrap_or(0);
    Ok(Json(Restored {
        time: snapshot.time,
        users: count("users"),
        files: count("files"),
    }))
}
