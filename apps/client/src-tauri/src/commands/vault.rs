// SPDX-License-Identifier: AGPL-3.0-or-later

//! First launch, unlock and lock, and the way the vault opens.
//!
//! Every vault has a password, which always opens it. Day to day it opens
//! with Windows Hello or a fingerprint, with a PIN typed in the app, or with
//! the password each time (see `app_core::vault`). Changing the way first
//! gets the key (the password when given, else the guarded copy, which
//! asks), then keeps it the new way and drops the others. Vaults from before
//! that opened without asking (`device`) are given a password when opened.

use app_core::crypto::{DatabaseKey, KdfParams};
use app_core::keystore::GuardedKeyStore;
use app_core::vault::{Encryption, Vault};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::biometric;
use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

/// How the vault opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Way {
    /// Not encrypted, chosen under "More options" on the first run (or a
    /// 0.1.0 vault). Security offers to encrypt it.
    Plain,
    /// An earlier vault this device opened without asking, no password.
    /// No longer made: it is given a password once open.
    Device,
    /// Windows Hello or a fingerprint releases the key.
    Guarded,
    /// A PIN typed in the app, a few tries.
    Pin,
    /// The password each time.
    Password,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent facts the ui reads as plain json fields"
)]
pub struct VaultStatus {
    exists: bool,
    encrypted: bool,
    unlocked: bool,
    way: Way,
    /// A password opens it: every vault but the old ones.
    has_password: bool,
    /// This device can verify the user (Windows Hello, a fingerprint).
    biometric_available: bool,
    /// This device can keep a key (its key store works), needed for a PIN.
    device_available: bool,
    /// A recovery key can open the vault.
    recovery: bool,
    /// Wrong PINs left before the password is needed, with the PIN way.
    pin_tries_left: Option<u8>,
}

#[tauri::command]
pub async fn vault_status(state: State<'_, AppState>) -> CommandResult<VaultStatus> {
    let unlocked = state.session().is_some();
    let guarded = biometric::guarded();
    let biometric_available = guarded.as_ref().is_some_and(GuardedKeyStore::available);
    let device = biometric::device_store();
    let device_available = device.is_some();
    if !Vault::exists(&state.dir) {
        return Ok(VaultStatus {
            exists: false,
            encrypted: false,
            unlocked,
            way: Way::Password,
            has_password: false,
            biometric_available,
            device_available,
            recovery: false,
            pin_tries_left: None,
        });
    }
    let vault = Vault::open(&state.dir)?;
    let name = vault.device_key_name();
    let pin_tries_left = device.and_then(|keys| vault.pin_tries_left(keys).ok().flatten());
    let way = if !vault.is_encrypted() {
        Way::Plain
    } else if !vault.has_password() {
        Way::Device
    } else if guarded
        .as_ref()
        .is_some_and(|g| g.contains(&name).unwrap_or(false))
    {
        Way::Guarded
    } else if pin_tries_left.is_some() {
        Way::Pin
    } else {
        Way::Password
    };
    Ok(VaultStatus {
        exists: true,
        encrypted: vault.is_encrypted(),
        unlocked,
        way,
        has_password: vault.has_password(),
        biometric_available,
        device_available,
        recovery: vault.has_recovery_key(),
        pin_tries_left,
    })
}

fn no_device() -> CommandError {
    CommandError::new("unsupported", "this device cannot keep a key")
}

fn no_biometric() -> CommandError {
    CommandError::new("unsupported", "this device has no biometric unlock yet")
}

fn needs_password() -> CommandError {
    CommandError::new("invalid", "a password is needed")
}

/// Brings the window forward, the system prompt otherwise opens behind it.
fn focus(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_focus();
    }
}

/// Runs a call that may wait for the user in a system prompt, or hash a
/// password, off the async worker threads.
async fn blocking<T: Send + 'static>(
    call: impl FnOnce() -> CommandResult<T> + Send + 'static,
) -> CommandResult<T> {
    tauri::async_runtime::spawn_blocking(call)
        .await
        .map_err(|e| CommandError::new("unknown", e.to_string()))?
}

/// First launch: the password, and how the vault opens day to day (`pin`
/// for the PIN way).
#[tauri::command]
pub async fn vault_create(
    app: AppHandle,
    state: State<'_, AppState>,
    way: Way,
    password: String,
    pin: Option<String>,
) -> CommandResult<()> {
    let dir = state.dir.clone();
    if way == Way::Plain {
        // no password, nothing to keep: anyone with the files reads them
        let (vault, store) = blocking(move || Ok(Vault::create(&dir, Encryption::Off)?)).await?;
        return state.open(vault, store);
    }
    if password.is_empty() {
        return Err(needs_password());
    }
    if way == Way::Device {
        return Err(CommandError::new("invalid", "new vaults have a password"));
    }
    focus(&app);
    let (vault, store) = blocking(move || {
        let (vault, store) = Vault::create(
            &dir,
            Encryption::On {
                password: &password,
                kdf: KdfParams::DEFAULT,
            },
        )?;
        let key = vault.key_with_password(&password)?;
        keep_quick_way(&vault, &key, way, pin.as_deref())?;
        Ok((vault, store))
    })
    .await?;
    state.open(vault, store)
}

/// Keeps the key the quick way chosen, and only that way.
fn keep_quick_way(
    vault: &Vault,
    key: &DatabaseKey,
    way: Way,
    pin: Option<&str>,
) -> CommandResult<()> {
    let device = biometric::device_store();
    match way {
        Way::Guarded => {
            let guarded = biometric::guarded().ok_or_else(no_biometric)?;
            // sealed first: the other copies go only once this one is kept
            vault.keep_key(key, &guarded)?;
            if let Some(keys) = device {
                let _ = vault.remove_pin(keys);
                let _ = vault.forget_on_device(keys);
            }
        }
        Way::Pin => {
            let keys = device.ok_or_else(no_device)?;
            let pin = pin.ok_or_else(|| CommandError::new("invalid", "a PIN is needed"))?;
            vault.set_pin(key, pin, keys, KdfParams::DEFAULT)?;
            let _ = vault.forget_on_device(keys);
            if let Some(guarded) = biometric::guarded() {
                let _ = vault.forget_on_device(&guarded);
            }
        }
        Way::Password => forget_kept(vault),
        Way::Plain | Way::Device => {
            return Err(CommandError::new("invalid", "not a way to choose"));
        }
    }
    Ok(())
}

/// Unlocks with the password, or without one an old vault this device
/// opened alone. Fails with `notRemembered` when the device has no key.
#[tauri::command]
pub async fn vault_unlock(
    state: State<'_, AppState>,
    password: Option<String>,
) -> CommandResult<()> {
    let dir = state.dir.clone();
    let (vault, store) = blocking(move || {
        let vault = Vault::open(&dir)?;
        let store = match (vault.is_encrypted(), password) {
            (false, _) => vault.unlock_plain()?,
            (true, Some(password)) => {
                let store = vault.unlock_with_password(&password)?;
                // 0.1.0 could remember the password's key, which opened the
                // vault without asking: that copy goes
                if let Some(keys) = biometric::device_store() {
                    let _ = vault.forget_on_device(keys);
                }
                store
            }
            (true, None) if vault.has_password() => return Err(needs_password()),
            (true, None) => {
                let keys = biometric::device_store().ok_or_else(no_device)?;
                vault.unlock_with_device(keys)?
            }
        };
        Ok((vault, store))
    })
    .await?;
    state.open(vault, store)
}

/// Unlocks with the PIN. `wrongPassword` for a wrong one with tries left,
/// `notRemembered` once the tries ran out: the password is needed.
#[tauri::command]
pub async fn vault_unlock_pin(state: State<'_, AppState>, pin: String) -> CommandResult<()> {
    let dir = state.dir.clone();
    let (vault, store) = blocking(move || {
        let vault = Vault::open(&dir)?;
        let keys = biometric::device_store().ok_or_else(no_device)?;
        let key = vault.key_with_pin(&pin, keys)?;
        let store = vault.unlock_with_key(&key)?;
        Ok((vault, store))
    })
    .await?;
    state.open(vault, store)
}

/// Unlocks after Windows Hello (or the platform equivalent). Fails with
/// `notRemembered` when the credential changed: the password is needed.
#[tauri::command]
pub async fn vault_unlock_biometric(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<()> {
    let guarded = biometric::guarded().ok_or_else(no_biometric)?;
    let vault = Vault::open(&state.dir)?;
    focus(&app);
    let (vault, store) = blocking(move || {
        let store = vault.unlock_with_device(&guarded)?;
        Ok((vault, store))
    })
    .await?;
    state.open(vault, store)
}

/// The key, from the password, else the copy this device keeps for an old
/// vault, else the guarded one (which asks Windows Hello, so call it off the
/// async threads).
fn current_key(vault: &Vault, password: Option<&str>) -> CommandResult<DatabaseKey> {
    if let Some(password) = password.filter(|p| !p.is_empty()) {
        return Ok(vault.key_with_password(password)?);
    }
    if let Some(keys) = biometric::device_store()
        && let Ok(key) = vault.key_from(keys)
    {
        return Ok(key);
    }
    if let Some(guarded) = biometric::guarded()
        && guarded.contains(&vault.device_key_name()).unwrap_or(false)
    {
        return Ok(vault.key_from(&guarded)?);
    }
    Err(needs_password())
}

/// Copies of the key this device keeps, all of them, the PIN too.
fn forget_kept(vault: &Vault) {
    if let Some(keys) = biometric::device_store() {
        let _ = vault.forget_on_device(keys);
        let _ = vault.remove_pin(keys);
    }
    if let Some(guarded) = biometric::guarded() {
        let _ = vault.forget_on_device(&guarded);
    }
}

/// The open session's copy of the vault reads `vault.json` again.
fn refresh(state: &AppState) -> CommandResult<()> {
    if let Some(session) = state.session().as_mut() {
        session.vault = Vault::open(&state.dir)?;
    }
    Ok(())
}

/// Changes how the vault opens day to day. `password` proves the right to
/// (or Windows Hello when that is the way), `new_password` gives a vault
/// without one its password, `pin` is the PIN way's.
#[tauri::command]
pub async fn vault_set_way(
    app: AppHandle,
    state: State<'_, AppState>,
    way: Way,
    password: Option<String>,
    new_password: Option<String>,
    pin: Option<String>,
) -> CommandResult<()> {
    let dir = state.dir.clone();
    focus(&app);
    blocking(move || {
        let mut vault = Vault::open(&dir)?;
        if !vault.is_encrypted() {
            return Err(CommandError::new("invalid", "encrypt the vault first"));
        }
        let key = current_key(&vault, password.as_deref())?;
        if !vault.has_password() {
            let new_password = new_password
                .filter(|p| !p.is_empty())
                .ok_or_else(needs_password)?;
            vault.set_password(&key, &new_password, KdfParams::DEFAULT)?;
        }
        keep_quick_way(&vault, &key, way, pin.as_deref())
    })
    .await?;
    refresh(&state)
}

/// Encrypts an old unencrypted vault, its key kept by this device until the
/// page gives it a password (`vault_set_way`).
#[tauri::command]
pub async fn vault_encrypt(state: State<'_, AppState>) -> CommandResult<()> {
    let keys = biometric::device_store().ok_or_else(no_device)?;
    let Some(session) = state.session().take() else {
        return Err(CommandError::locked());
    };
    let mut vault = session.vault;
    drop(session.blobs);
    let store = vault.encrypt(session.store, keys)?;
    state.open(vault, store)
}

#[tauri::command]
pub async fn vault_lock(state: State<'_, AppState>) -> CommandResult<()> {
    state.close();
    Ok(())
}

/// Deletes everything this device keeps: the vault (notes, files, history,
/// the sign-in to a server), the imported themes, and the keys
/// this device kept for it. The page then starts the first launch setup.
/// Works locked too: a vault that no longer opens can still go.
#[tauri::command]
pub async fn vault_reset(state: State<'_, AppState>) -> CommandResult<()> {
    state.close();
    // stale keys would make a new vault look kept already
    if let Ok(vault) = Vault::open(&state.dir) {
        forget_kept(&vault);
    }
    Ok(Vault::destroy(&state.dir)?)
}

#[tauri::command]
pub async fn vault_change_password(
    state: State<'_, AppState>,
    old: String,
    new: String,
) -> CommandResult<()> {
    let mut guard = state.session();
    let session = guard.as_mut().ok_or_else(CommandError::locked)?;
    session
        .vault
        .change_password(&old, &new, KdfParams::DEFAULT)?;
    Ok(())
}

/// Makes a new recovery key for the vault, replacing any earlier one, and
/// returns it to show once. `password` when the vault has one.
#[tauri::command]
pub async fn vault_create_recovery(
    app: AppHandle,
    state: State<'_, AppState>,
    password: Option<String>,
) -> CommandResult<String> {
    let dir = state.dir.clone();
    focus(&app);
    let code = blocking(move || {
        let mut vault = Vault::open(&dir)?;
        let key = current_key(&vault, password.as_deref())?;
        Ok(vault
            .create_recovery_key_for(&key, KdfParams::DEFAULT)?
            .to_string())
    })
    .await?;
    refresh(&state)?;
    Ok(code)
}

#[tauri::command]
pub async fn vault_remove_recovery(
    app: AppHandle,
    state: State<'_, AppState>,
    password: Option<String>,
) -> CommandResult<()> {
    let dir = state.dir.clone();
    focus(&app);
    blocking(move || {
        let mut vault = Vault::open(&dir)?;
        let key = current_key(&vault, password.as_deref())?;
        Ok(vault.remove_recovery_key_for(&key)?)
    })
    .await?;
    refresh(&state)
}

/// The password is forgotten, or an old vault's device key is gone: opens
/// with the recovery key and sets a new password, the only way from then on
/// until a quicker one is chosen again.
#[tauri::command]
pub async fn vault_recover(
    state: State<'_, AppState>,
    code: String,
    new_password: String,
) -> CommandResult<()> {
    if new_password.is_empty() {
        return Err(needs_password());
    }
    let dir = state.dir.clone();
    let (vault, store) = blocking(move || {
        let mut vault = Vault::open(&dir)?;
        let key = vault.key_with_recovery(&code)?;
        vault.set_password(&key, &new_password, KdfParams::DEFAULT)?;
        forget_kept(&vault);
        let store = vault.unlock_with_key(&key)?;
        Ok((vault, store))
    })
    .await?;
    state.open(vault, store)
}
