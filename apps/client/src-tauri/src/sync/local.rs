// SPDX-License-Identifier: AGPL-3.0-or-later

//! Sync with this person's other devices on the same network (`app-peer`),
//! wired to the open vault. A watcher thread starts and stops the node with
//! the vault and, on a phone, with the app being in front, and starts a
//! round when something was written here. The ui hears `local-sync-status`.
//!
//! A vault uses a server or its devices, never both: the commands here
//! refuse while a server is linked, and signing in refuses while a device
//! is paired.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use app_core::blobs::BlobStore;
use app_core::store::Store;
use app_peer::{Host, Live, Me, Node, PairingStatus};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

/// How often the watcher looks for writes and for the vault.
const WATCH_EVERY: Duration = Duration::from_millis(500);

/// The node's view of the app.
pub struct LocalHost {
    app: AppHandle,
}

impl Host for LocalHost {
    fn with<T>(
        &self,
        f: impl FnOnce(&mut Store, &BlobStore) -> app_core::Result<T>,
    ) -> Option<app_core::Result<T>> {
        let state = self.app.state::<AppState>();
        let mut guard = state.session();
        let session = guard.as_mut()?;
        Some(f(&mut session.store, &session.blobs))
    }

    fn changed(&self) {
        let _ = self.app.emit("sync-changed", ());
    }

    fn files_arrived(&self) {
        let _ = self.app.emit("files-changed", ());
    }

    fn status_changed(&self) {
        let local = self.app.state::<Local>();
        let status = local.status(&self.app.state::<AppState>());
        let _ = self.app.emit("local-sync-status", status);
    }

    fn fetch_up_to(&self) -> u64 {
        self.app.state::<AppState>().sync.auto_fetch()
    }

    fn default_name(&self) -> String {
        default_name()
    }
}

/// The computer's name, or what kind of device this is. Asked once.
fn default_name() -> String {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(read_default_name).clone()
}

fn read_default_name() -> String {
    let named = if cfg!(target_os = "linux") {
        std::fs::read_to_string("/proc/sys/kernel/hostname").ok()
    } else if cfg!(windows) {
        std::env::var("COMPUTERNAME").ok()
    } else {
        None
    };
    #[cfg(target_os = "android")]
    let named = named.or_else(android::model);
    named
        .map(|n| n.trim().to_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| {
            match platform() {
                "android" => "Android phone",
                "macos" => "Mac",
                "windows" => "Windows PC",
                _ => "Linux computer",
            }
            .to_owned()
        })
}

fn platform() -> &'static str {
    if cfg!(target_os = "android") {
        "android"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else {
        "linux"
    }
}

/// The node, and whether the app is in front (always on a computer).
pub struct Local {
    node: OnceLock<Node<LocalHost>>,
    foreground: AtomicBool,
}

impl Default for Local {
    fn default() -> Self {
        Self {
            node: OnceLock::new(),
            foreground: AtomicBool::new(true),
        }
    }
}

/// A paired device, for the settings.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalDevice {
    /// Hex of its key, how the commands name it.
    key: String,
    name: String,
    platform: String,
    version: String,
    last_sync_at: Option<i64>,
    /// `syncing`, `synced`, `waiting` (changes here it has not seen),
    /// `away`, `needsUpdate`, `updateThis` or `failed`.
    state: &'static str,
    /// The other version, or why it failed.
    detail: Option<String>,
    /// Seen but out of reach for a while: a firewall may be in the way.
    blocked: bool,
}

/// Where local sync stands.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalStatus {
    /// The vault is open.
    open: bool,
    /// This device's name.
    name: String,
    /// A server is linked: local sync is off.
    server_linked: bool,
    running: bool,
    /// The port this device listens on, for a firewall rule.
    port: Option<u16>,
    discovery_error: Option<String>,
    pairing: Option<PairingView>,
    devices: Vec<LocalDevice>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct PairingView {
    code: String,
    seconds_left: u64,
    address: Option<String>,
    failed: Option<&'static str>,
    paired: Option<String>,
}

impl From<PairingStatus> for PairingView {
    fn from(p: PairingStatus) -> Self {
        Self {
            code: p.code,
            seconds_left: p.seconds_left,
            address: p.address,
            failed: p.failed,
            paired: p.paired,
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    (0..text.len())
        .step_by(2)
        .map(|i| {
            text.get(i..i + 2)
                .and_then(|b| u8::from_str_radix(b, 16).ok())
        })
        .collect()
}

impl Local {
    fn node(&self) -> Option<&Node<LocalHost>> {
        self.node.get()
    }

    /// Starts the node and its watcher.
    pub fn start(&self, app: &AppHandle) {
        let me = Me {
            platform: platform().into(),
            version: app_brand::VERSION.into(),
        };
        let host = Arc::new(LocalHost { app: app.clone() });
        // debug builds can run as on a network that drops multicast, to test
        // reaching paired devices at the addresses kept
        let node = if cfg!(debug_assertions) && std::env::var_os("APP_NO_DISCOVERY").is_some() {
            Node::without_discovery(host, me)
        } else {
            Node::new(host, me)
        };
        if self.node.set(node).is_err() {
            return;
        }
        let app = app.clone();
        let spawned = std::thread::Builder::new()
            .name("local-sync".into())
            .spawn(move || watch(&app));
        debug_assert!(spawned.is_ok(), "the local sync watcher starts");
    }

    fn status(&self, state: &AppState) -> LocalStatus {
        let live = self.node().map(Node::status);
        let read = state.quietly(|s| {
            Ok((
                s.store.peer_name()?,
                s.store.server_linked()?,
                s.store.peer_devices()?,
                s.store.peer_last_change()?,
            ))
        });
        let Ok((name, server_linked, devices, last_change)) = read else {
            return LocalStatus {
                open: false,
                name: default_name(),
                server_linked: false,
                running: false,
                port: None,
                discovery_error: None,
                pairing: None,
                devices: Vec::new(),
            };
        };
        let devices = devices
            .into_iter()
            .filter(app_core::store::PeerDevice::active)
            .map(|d| {
                let key = hex(&d.key);
                let now = live.as_ref().and_then(|l| l.live.get(&key));
                let behind = last_change.is_some_and(|c| d.last_sync_at.is_none_or(|s| s < c));
                let (state, detail) = match now {
                    Some(Live::Syncing) => ("syncing", None),
                    Some(Live::Synced) => ("synced", None),
                    Some(Live::NeedsUpdate { version }) => ("needsUpdate", Some(version.clone())),
                    Some(Live::UpdateThis { version }) => ("updateThis", Some(version.clone())),
                    Some(Live::Failed { error }) => ("failed", Some(error.clone())),
                    None if behind => ("waiting", None),
                    None => ("away", None),
                };
                LocalDevice {
                    blocked: live.as_ref().is_some_and(|l| l.blocked.contains(&key)),
                    key,
                    name: d.name,
                    platform: d.platform,
                    version: d.version,
                    last_sync_at: d.last_sync_at,
                    state,
                    detail,
                }
            })
            .collect();
        LocalStatus {
            open: true,
            name: name.unwrap_or_else(default_name),
            server_linked,
            running: live.as_ref().is_some_and(|l| l.running),
            port: live.as_ref().and_then(|l| l.port),
            discovery_error: live.as_ref().and_then(|l| l.discovery_error.clone()),
            pairing: live.and_then(|l| l.pairing).map(PairingView::from),
            devices,
        }
    }
}

/// Follows the vault and the writes made here.
fn watch(app: &AppHandle) {
    let local = app.state::<Local>();
    let Some(node) = local.node() else { return };
    let mut wanted = false;
    let mut last_change: Option<i64> = None;
    let mut sent: Option<LocalStatus> = None;
    let mut ticks = 0u32;
    loop {
        let state = app.state::<AppState>();
        let change = state.quietly(|s| s.store.peer_last_change());
        let open = change.is_ok();
        let now_wanted = open && local.foreground.load(Ordering::SeqCst);
        if now_wanted != wanted {
            wanted = now_wanted;
            #[cfg(target_os = "android")]
            android::multicast(wanted);
            node.set_wanted(wanted);
        }
        if let Ok(change) = change {
            if last_change.is_some() && change != last_change {
                node.kick();
            }
            last_change = change;
        }
        // the code's seconds left, a phone that went away, a device that
        // reconnects: the status is sent again when it differs
        ticks = ticks.wrapping_add(1);
        if ticks.is_multiple_of(2) {
            let status = local.status(&state);
            if sent.as_ref() != Some(&status) {
                let _ = app.emit("local-sync-status", status.clone());
                sent = Some(status);
            }
        }
        std::thread::sleep(WATCH_EVERY);
    }
}

/// How long opening a file waits for a paired device to send it.
const FETCH_FOR: Duration = Duration::from_secs(30);

/// A file a note uses, asked of the connected devices. Waits for it.
///
/// # Errors
///
/// `notFound` when no device is connected or none sent it in time.
pub fn fetch(app: &AppHandle, blobs: &BlobStore, sha256: &str) -> CommandResult<()> {
    let local = app.state::<Local>();
    let asked = local.node().is_some_and(|n| n.want(sha256));
    if asked {
        let end = std::time::Instant::now() + FETCH_FOR;
        while std::time::Instant::now() < end {
            if blobs.has(sha256).unwrap_or(false) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    Err(CommandError::new(
        "notFound",
        "no paired device that has this file is connected",
    ))
}

fn node<'a>(local: &'a State<'_, Local>) -> CommandResult<&'a Node<LocalHost>> {
    local
        .node()
        .ok_or_else(|| CommandError::new("unavailable", "local sync did not start"))
}

fn peer_error(err: app_peer::Error) -> CommandError {
    use app_peer::Error as E;
    match err {
        E::WrongCode => CommandError::new("wrongCode", err.to_string()),
        E::NotFound => CommandError::new("notFound", err.to_string()),
        E::Locked => CommandError::locked(),
        E::Store(app_core::Error::Invalid(why)) if why == app_core::store::SERVER_LINKED => {
            CommandError::new("serverLinked", why)
        }
        E::Store(other) => other.into(),
        other => CommandError::new("network", other.to_string()),
    }
}

#[tauri::command]
pub async fn local_sync_status(
    state: State<'_, AppState>,
    local: State<'_, Local>,
) -> CommandResult<LocalStatus> {
    Ok(local.status(&state))
}

#[tauri::command]
pub async fn local_sync_set_name(
    state: State<'_, AppState>,
    local: State<'_, Local>,
    name: String,
) -> CommandResult<LocalStatus> {
    state.quietly(|s| s.store.set_peer_name(&name))?;
    Ok(local.status(&state))
}

/// Shows a code for another device to type.
#[tauri::command]
pub async fn local_sync_pair_start(
    state: State<'_, AppState>,
    local: State<'_, Local>,
) -> CommandResult<LocalStatus> {
    node(&local)?.start_pairing().map_err(peer_error)?;
    Ok(local.status(&state))
}

#[tauri::command]
pub async fn local_sync_pair_cancel(
    state: State<'_, AppState>,
    local: State<'_, Local>,
) -> CommandResult<LocalStatus> {
    node(&local)?.cancel_pairing();
    Ok(local.status(&state))
}

/// Pairs with the device showing `code`. Its name.
#[tauri::command]
pub async fn local_sync_pair_code(
    local: State<'_, Local>,
    code: String,
    address: Option<String>,
) -> CommandResult<String> {
    let node = node(&local)?.clone();
    tauri::async_runtime::spawn_blocking(move || {
        node.pair_with_code(&code, address.as_deref().filter(|a| !a.trim().is_empty()))
    })
    .await
    .map_err(|e| CommandError::new("network", e.to_string()))?
    .map_err(peer_error)
}

/// Forgets a device: it stops syncing with every device here. The notes stay.
#[tauri::command]
pub async fn local_sync_forget(
    state: State<'_, AppState>,
    local: State<'_, Local>,
    key: String,
) -> CommandResult<LocalStatus> {
    let key = unhex(&key).ok_or_else(|| CommandError::new("invalid", "not a device key"))?;
    state.quietly(|s| s.store.peer_forget(&key))?;
    node(&local)?.forget(&key);
    Ok(local.status(&state))
}

#[tauri::command]
pub async fn local_sync_now(local: State<'_, Local>) -> CommandResult<()> {
    node(&local)?.sync_now();
    Ok(())
}

/// The app came to the front or went away. A phone syncs only in front.
#[tauri::command]
pub async fn local_sync_foreground(local: State<'_, Local>, visible: bool) -> CommandResult<()> {
    if cfg!(target_os = "android") {
        local.foreground.store(visible, Ordering::SeqCst);
        if visible && let Some(node) = local.node() {
            node.sync_now();
        }
    }
    Ok(())
}

/// Whether the app may reach the local network: `granted`, `denied` or
/// `prompt`. With `ask`, asks the person first. Always granted on a
/// computer.
#[tauri::command]
pub async fn local_sync_permission(ask: bool) -> CommandResult<String> {
    #[cfg(target_os = "android")]
    return android::permission(ask);
    #[cfg(not(target_os = "android"))]
    {
        let _ = ask;
        Ok("granted".into())
    }
}

#[cfg(target_os = "android")]
mod android {
    //! `LocalNetworkPlugin` (LocalNetwork.kt): the multicast lock, the
    //! local network permission and the phone's model.

    use std::sync::OnceLock;

    use serde::{Deserialize, Serialize};
    use tauri::Wry;
    use tauri::plugin::{PluginHandle, TauriPlugin};

    use crate::error::{CommandError, CommandResult};

    static HANDLE: OnceLock<PluginHandle<Wry>> = OnceLock::new();

    /// The in-app plugin. No permission names it, so the page cannot call it.
    pub fn plugin() -> TauriPlugin<Wry> {
        tauri::plugin::Builder::new("local-network")
            .setup(|_app, api| {
                let handle =
                    api.register_android_plugin("com.synaelle.ficelle.dev", "LocalNetworkPlugin")?;
                let _ = HANDLE.set(handle);
                Ok(())
            })
            .build()
    }

    #[derive(Serialize)]
    struct Ask {
        ask: bool,
    }

    #[derive(Deserialize)]
    struct Permission {
        state: String,
    }

    #[derive(Deserialize)]
    struct Model {
        model: String,
    }

    /// Takes or lets go of the multicast lock mDNS needs.
    pub fn multicast(on: bool) {
        if let Some(handle) = HANDLE.get() {
            let command = if on {
                "acquireMulticast"
            } else {
                "releaseMulticast"
            };
            let _ = handle.run_mobile_plugin::<()>(command, ());
        }
    }

    pub fn permission(ask: bool) -> CommandResult<String> {
        let handle = HANDLE
            .get()
            .ok_or_else(|| CommandError::new("unavailable", "the plugin is not ready"))?;
        handle
            .run_mobile_plugin::<Permission>("permission", Ask { ask })
            .map(|p| p.state)
            .map_err(|e| CommandError::new("unavailable", e.to_string()))
    }

    pub fn model() -> Option<String> {
        HANDLE
            .get()?
            .run_mobile_plugin::<Model>("model", ())
            .ok()
            .map(|m| m.model)
    }
}

#[cfg(target_os = "android")]
pub use android::plugin;
