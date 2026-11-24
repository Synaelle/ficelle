// SPDX-License-Identifier: AGPL-3.0-or-later

//! First launch, unlock and lock, and the way the vault opens, as the core
//! has them (`app_core::commands::vault`), with this device's key stores.

use app_core::commands::vault::{self, VaultStatus, Way};
use app_core::crypto::KdfParams;
use app_core::vault::Vault;
use tauri::{AppHandle, Manager, State};

use crate::biometric;
use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

/// Whether the app may write in the vault's folder: a read-only one (a disk
/// mounted read-only, a full one, a copy without rights) opens nothing.
fn writable(dir: &std::path::Path) -> bool {
    let probe = dir.join(".write-check");
    let made = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    made
}

#[tauri::command]
pub async fn vault_status(state: State<'_, AppState>) -> CommandResult<VaultStatus> {
    state.check_alone()?;
    let unlocked = state.session().is_some();
    let keys = biometric::keys();
    // the page says to put it back, a new vault would not take the notes in
    if Vault::meta_missing(&state.dir) {
        return Err(CommandError::new(
            "vaultMissing",
            state.dir.display().to_string(),
        ));
    }
    if !Vault::exists(&state.dir) {
        return Ok(VaultStatus::none(unlocked, &keys));
    }
    let vault = match Vault::open(&state.dir) {
        Ok(vault) => vault,
        // the page names the folder and says how to put it right
        Err(app_core::Error::Corrupt(_)) => {
            return Err(CommandError::new(
                "vaultDamaged",
                state.dir.display().to_string(),
            ));
        }
        Err(e) => return Err(e.into()),
    };
    // said before any password is asked, and nothing is made in its place
    if !unlocked && vault.notes_missing() {
        return Err(CommandError::new(
            "notesMissing",
            state.dir.display().to_string(),
        ));
    }
    if !unlocked && !writable(&state.dir) {
        return Err(CommandError::new(
            "readOnly",
            state.dir.display().to_string(),
        ));
    }
    Ok(VaultStatus::of(&vault, unlocked, &keys))
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
    state.check_alone()?;
    let dir = state.dir.clone();
    if way != Way::Plain {
        focus(&app);
    }
    let (vault, store) = blocking(move || {
        let disk = std::sync::Arc::new(app_core::disk::NativeDisk);
        let keys = biometric::keys();
        Ok(vault::create(
            disk,
            &dir,
            way,
            &password,
            pin.as_deref(),
            &keys,
        )?)
    })
    .await?;
    state.open(vault, store)
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
        let store = vault::unlock(&vault, password.as_deref(), &biometric::keys())?;
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
        let store = vault::unlock_with_pin(&vault, &pin, &biometric::keys())?;
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
    let guarded = biometric::guarded().ok_or_else(|| {
        CommandError::new("unsupported", "this device has no biometric unlock yet")
    })?;
    let vault = Vault::open(&state.dir)?;
    focus(&app);
    let (vault, store) = blocking(move || {
        let store = vault.unlock_with_device(&guarded)?;
        Ok((vault, store))
    })
    .await?;
    state.open(vault, store)
}

/// The open session's copy of the vault reads `vault.json` again.
fn refresh(state: &AppState) -> CommandResult<()> {
    if let Some(session) = state.session().as_mut() {
        session.vault = session.vault.reopen()?;
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
    let (given, kept) = blocking(move || {
        let mut vault = Vault::open(&dir)?;
        Ok(vault::set_way(
            &mut vault,
            way,
            password.as_deref(),
            new_password,
            pin.as_deref(),
            &biometric::keys(),
        )?)
    })
    .await?;
    refresh(&state)?;
    if let Some(password) = given {
        // backups out of reach now (a drive unplugged) stay as they are,
        // and none are made until they are set up again with a password
        let _ = super::backups::protect(app, password).await;
    }
    // the password stays set when the quick way fails: the backups take it
    // all the same
    Ok(kept?)
}

/// Encrypts an unencrypted vault with `password`, no system keychain needed;
/// a quick way is added after (`vault_set_way`). Backups made without a
/// password take it too. Without a password the key is kept by this device
/// until the page gives it one. A failed try leaves the notes open as they
/// were.
#[tauri::command]
pub async fn vault_encrypt(
    app: AppHandle,
    state: State<'_, AppState>,
    password: Option<String>,
) -> CommandResult<()> {
    let password = password.filter(|p| !p.is_empty());
    // looked up before the session is taken out: a missing one fails here
    let device = vault::encryption_keys(password.as_deref(), &biometric::keys())?;
    let Some(session) = state.session().take() else {
        return Err(CommandError::locked());
    };
    drop(session.blobs);
    match vault::encrypt(session.vault, session.store, password.as_deref(), device) {
        Ok((vault, store)) => state.open(vault, store)?,
        Err(failed) => {
            if let Some((vault, store)) = failed.open {
                state.open(vault, store)?;
            }
            return Err(failed.error.into());
        }
    }
    if let Some(password) = password {
        // as in vault_set_way: backups out of reach stay as they are
        let _ = super::backups::protect(app, password).await;
    }
    Ok(())
}

#[tauri::command]
pub async fn vault_lock(app: tauri::AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    state.close();
    #[cfg(desktop)]
    crate::sync::files::clean_opened_copies(&app);
    // every window locks, not only the one that asked
    let _ = tauri::Emitter::emit(&app, "vault-locked", ());
    Ok(())
}

/// Deletes everything this device keeps: the vault (notes, files, history,
/// the sign-in to a server), and the keys
/// this device kept for it. The page then starts the first launch setup.
/// Works locked too: a vault that no longer opens can still go.
#[tauri::command]
pub async fn vault_reset(state: State<'_, AppState>) -> CommandResult<()> {
    state.check_alone()?;
    state.close();
    // stale keys would make a new vault look kept already
    let forgotten = match Vault::open(&state.dir) {
        Ok(vault) => vault::forget_kept(&vault, &biometric::keys()),
        Err(_) => Ok(()),
    };
    // the notes go even when a key copy would not: the page shows that error
    Vault::destroy(&state.dir)?;
    Ok(forgotten?)
}

/// Starts again from the lock screen: the vault's files move into a dated
/// folder beside them, never deleted, since a damaged database still holds
/// most notes for a recovery tool. The keys this device kept stay, they open
/// that copy if it is put back. Returns the folder. The page then starts the
/// first launch setup.
#[tauri::command]
pub async fn vault_set_aside(state: State<'_, AppState>) -> CommandResult<String> {
    state.check_alone()?;
    state.close();
    let now = app_core::clock::Clock::now(&app_core::clock::SystemClock);
    let aside = Vault::set_aside_on(&app_core::disk::NativeDisk, &state.dir, now)?;
    Ok(aside.display().to_string())
}

#[tauri::command]
pub async fn vault_change_password(
    app: AppHandle,
    state: State<'_, AppState>,
    old: String,
    new: String,
) -> CommandResult<()> {
    {
        let mut guard = state.session();
        let session = guard.as_mut().ok_or_else(CommandError::locked)?;
        session
            .vault
            .change_password(&old, &new, KdfParams::DEFAULT)?;
    }
    // the next backups are sealed with the new one
    super::backups::protect(app, new).await
}

/// Whether `password` opens these notes, nothing else done: a backup can
/// then use the same password, typed once and checked here.
#[tauri::command]
pub async fn vault_check_password(
    state: State<'_, AppState>,
    password: String,
) -> CommandResult<bool> {
    let dir = state.dir.clone();
    blocking(move || {
        let vault = Vault::open(&dir)?;
        Ok(vault::check_password(&vault, &password))
    })
    .await
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
        Ok(vault::create_recovery(
            &mut vault,
            password.as_deref(),
            &biometric::keys(),
        )?)
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
        Ok(vault::remove_recovery(
            &mut vault,
            password.as_deref(),
            &biometric::keys(),
        )?)
    })
    .await?;
    refresh(&state)
}

/// The password is forgotten, or an old vault's device key is gone: opens
/// with the recovery key and sets a new password, the only way from then on
/// until a quicker one is chosen again.
#[tauri::command]
pub async fn vault_recover(
    app: AppHandle,
    state: State<'_, AppState>,
    code: String,
    new_password: String,
) -> CommandResult<()> {
    if new_password.is_empty() {
        return Err(vault::needs_password().into());
    }
    let dir = state.dir.clone();
    let password = new_password.clone();
    let (vault, store) = blocking(move || {
        let mut vault = Vault::open(&dir)?;
        let store = vault::recover(&mut vault, &code, &password, &biometric::keys())?;
        Ok((vault, store))
    })
    .await?;
    state.open(vault, store)?;
    // the forgotten password no longer seals the next backups; the notes
    // are open either way, as in vault_set_way
    let _ = super::backups::protect(app, new_password).await;
    Ok(())
}
