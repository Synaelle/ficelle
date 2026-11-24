// SPDX-License-Identifier: AGPL-3.0-or-later

//! First launch, unlock, encryption and recovery, and the way the vault
//! opens.
//!
//! Every vault has a password, which always opens it. Day to day it opens
//! with Windows Hello or a fingerprint, with a PIN typed in the app, or with
//! the password each time (see [`crate::vault`]). Changing the way first
//! gets the key (the password when given, else the guarded copy, which
//! asks), then keeps it the new way and drops the others. Vaults from before
//! that opened without asking (`device`) are given a password when opened.
//! The web version has no key store, so a password or nothing.

use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::crypto::{DatabaseKey, KdfParams};
use crate::disk::Disk;
use crate::error::{Error, Result};
use crate::keystore::{Authenticator, GuardedKeyStore, KeyStore};
use crate::store::Store;
use crate::vault::{Encryption, Vault};

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

/// The key store that releases a key only once the platform verified the
/// user.
pub type Guarded<'a> = GuardedKeyStore<&'a dyn Authenticator, &'a dyn KeyStore>;

/// Where this device can keep a vault's key. The web version has neither.
#[derive(Default)]
pub struct Keys<'a> {
    /// The device's own key store, `None` when it has none that works.
    pub device: Option<&'a dyn KeyStore>,
    /// Behind Windows Hello or a fingerprint, `None` where the platform has
    /// no verification wired in.
    pub guarded: Option<Guarded<'a>>,
}

/// The vault as the lock screen and the security settings read it.
#[derive(Debug, Serialize)]
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
    /// This check found an encryption a crash cut short: the notes stayed
    /// unencrypted.
    encryption_undone: bool,
}

impl VaultStatus {
    /// No vault yet: the page starts the first launch setup.
    pub fn none(unlocked: bool, keys: &Keys<'_>) -> Self {
        Self {
            exists: false,
            encrypted: false,
            unlocked,
            way: Way::Password,
            has_password: false,
            biometric_available: biometric_available(keys),
            device_available: keys.device.is_some(),
            recovery: false,
            pin_tries_left: None,
            encryption_undone: false,
        }
    }

    /// The vault as it stands, and how it opens on this device.
    pub fn of(vault: &Vault, unlocked: bool, keys: &Keys<'_>) -> Self {
        let name = vault.device_key_name();
        let pin_tries_left = keys
            .device
            .and_then(|device| vault.pin_tries_left(device).ok().flatten());
        let way = if !vault.is_encrypted() {
            Way::Plain
        } else if !vault.has_password() {
            Way::Device
        } else if keys
            .guarded
            .as_ref()
            .is_some_and(|g| g.contains(&name).unwrap_or(false))
        {
            Way::Guarded
        } else if pin_tries_left.is_some() {
            Way::Pin
        } else {
            Way::Password
        };
        Self {
            exists: true,
            encrypted: vault.is_encrypted(),
            unlocked,
            way,
            has_password: vault.has_password(),
            biometric_available: biometric_available(keys),
            device_available: keys.device.is_some(),
            recovery: vault.has_recovery_key(),
            pin_tries_left,
            encryption_undone: vault.encryption_undone(),
        }
    }
}

fn biometric_available(keys: &Keys<'_>) -> bool {
    keys.guarded
        .as_ref()
        .is_some_and(GuardedKeyStore::available)
}

/// A password was needed and none was given.
pub fn needs_password() -> Error {
    Error::Invalid("a password is needed".into())
}

fn no_device() -> Error {
    Error::Unsupported("this device cannot keep a key")
}

fn no_biometric() -> Error {
    Error::Unsupported("this device has no biometric unlock yet")
}

/// First launch: the vault in `dir`, with `password` and the way it opens
/// day to day (`pin` for the PIN way), or unencrypted.
///
/// # Errors
///
/// [`Error::Invalid`] without a password or for the old `device` way,
/// [`Error::Unsupported`] for a way this device cannot keep, file, database
/// and key store errors.
pub fn create(
    disk: Arc<dyn Disk>,
    dir: &Path,
    way: Way,
    password: &str,
    pin: Option<&str>,
    keys: &Keys<'_>,
) -> Result<(Vault, Store)> {
    if way == Way::Plain {
        // no password, nothing to keep: anyone with the files reads them
        return Vault::create_on(disk, dir, Encryption::Off);
    }
    if password.is_empty() {
        return Err(needs_password());
    }
    match way {
        Way::Device => return Err(Error::Invalid("new vaults have a password".into())),
        // said before a vault is made that could not open the way chosen
        Way::Guarded if keys.guarded.is_none() => return Err(no_biometric()),
        Way::Pin if keys.device.is_none() => return Err(no_device()),
        _ => {}
    }
    let (vault, store) = Vault::create_on(
        disk,
        dir,
        Encryption::On {
            password,
            kdf: KdfParams::DEFAULT,
        },
    )?;
    let key = vault.key_with_password(password)?;
    keep_quick_way(&vault, &key, way, pin, keys)?;
    Ok((vault, store))
}

/// Keeps the key the quick way chosen, and only that way.
fn keep_quick_way(
    vault: &Vault,
    key: &DatabaseKey,
    way: Way,
    pin: Option<&str>,
    keys: &Keys<'_>,
) -> Result<()> {
    match way {
        Way::Guarded => {
            let guarded = keys.guarded.as_ref().ok_or_else(no_biometric)?;
            // sealed first: the other copies go only once this one is kept
            vault.keep_key(key, guarded)?;
            if let Some(device) = keys.device {
                vault.remove_pin(device)?;
                vault.forget_on_device(device)?;
            }
        }
        Way::Pin => {
            let device = keys.device.ok_or_else(no_device)?;
            let pin = pin.ok_or_else(|| Error::Invalid("a PIN is needed".into()))?;
            vault.set_pin(key, pin, device, KdfParams::DEFAULT)?;
            vault.forget_on_device(device)?;
            if let Some(guarded) = &keys.guarded {
                vault.forget_on_device(guarded)?;
            }
        }
        Way::Password => forget_kept(vault, keys)?,
        Way::Plain | Way::Device => {
            return Err(Error::Invalid("not a way to choose".into()));
        }
    }
    Ok(())
}

/// Unlocks with the password, or without one an old vault this device
/// opened alone.
///
/// # Errors
///
/// [`Error::NotRemembered`] when the device has no key, a wrong password,
/// database errors.
pub fn unlock(vault: &Vault, password: Option<&str>, keys: &Keys<'_>) -> Result<Store> {
    match (vault.is_encrypted(), password) {
        (false, _) => vault.unlock_plain(),
        (true, Some(password)) => {
            let store = vault.unlock_with_password(password)?;
            // 0.1.0 could remember the password's key, which opened the
            // vault without asking: that copy goes
            if let Some(device) = keys.device {
                let _ = vault.forget_on_device(device);
            }
            Ok(store)
        }
        (true, None) if vault.has_password() => Err(needs_password()),
        (true, None) => vault.unlock_with_device(keys.device.ok_or_else(no_device)?),
    }
}

/// Unlocks with the PIN.
///
/// # Errors
///
/// [`Error::WrongPassword`] for a wrong one with tries left,
/// [`Error::NotRemembered`] once the tries ran out: the password is needed.
pub fn unlock_with_pin(vault: &Vault, pin: &str, keys: &Keys<'_>) -> Result<Store> {
    let key = vault.key_with_pin(pin, keys.device.ok_or_else(no_device)?)?;
    vault.unlock_with_key(&key)
}

/// The key, from the password, else the copy this device keeps for an old
/// vault, else the guarded one (which asks Windows Hello, so call it off
/// the async threads).
fn current_key(vault: &Vault, password: Option<&str>, keys: &Keys<'_>) -> Result<DatabaseKey> {
    if let Some(password) = password.filter(|p| !p.is_empty()) {
        return vault.key_with_password(password);
    }
    if let Some(device) = keys.device
        && let Ok(key) = vault.key_from(device)
    {
        return Ok(key);
    }
    if let Some(guarded) = &keys.guarded
        && guarded.contains(&vault.device_key_name()).unwrap_or(false)
    {
        return vault.key_from(guarded);
    }
    Err(needs_password())
}

/// Forgets every copy of the key this device keeps, the PIN too. Each one
/// is tried even when another fails; the first failure is returned, so the
/// page never says a copy is gone when it is not.
///
/// # Errors
///
/// The first key store error.
pub fn forget_kept(vault: &Vault, keys: &Keys<'_>) -> Result<()> {
    let mut results = Vec::new();
    if let Some(device) = keys.device {
        results.push(vault.forget_on_device(device));
        results.push(vault.remove_pin(device));
    }
    if let Some(guarded) = &keys.guarded {
        results.push(vault.forget_on_device(guarded));
    }
    results.into_iter().collect()
}

/// Changes how the vault opens day to day. `password` proves the right to
/// (or Windows Hello when that is the way), `new_password` gives a vault
/// without one its password, `pin` is the PIN way's. Returns the password
/// given, if any, and how keeping the new way went: the password stays set
/// when the quick way fails.
///
/// # Errors
///
/// [`Error::Invalid`] for an unencrypted vault or a missing password, key
/// and file errors.
pub fn set_way(
    vault: &mut Vault,
    way: Way,
    password: Option<&str>,
    new_password: Option<String>,
    pin: Option<&str>,
    keys: &Keys<'_>,
) -> Result<(Option<String>, Result<()>)> {
    if !vault.is_encrypted() {
        return Err(Error::Invalid("encrypt the vault first".into()));
    }
    let key = current_key(vault, password, keys)?;
    let mut given = None;
    if !vault.has_password() {
        let new_password = new_password
            .filter(|p| !p.is_empty())
            .ok_or_else(needs_password)?;
        vault.set_password(&key, &new_password, KdfParams::DEFAULT)?;
        given = Some(new_password);
    }
    Ok((given, keep_quick_way(vault, &key, way, pin, keys)))
}

/// The key store an encryption without a password keeps the key in, looked
/// up before the vault is closed for it: none needed with a password.
///
/// # Errors
///
/// [`Error::Unsupported`] without a password on a device with no key store.
pub fn encryption_keys<'a>(
    password: Option<&str>,
    keys: &Keys<'a>,
) -> Result<Option<&'a dyn KeyStore>> {
    match password {
        Some(_) => Ok(None),
        None => keys.device.ok_or_else(no_device).map(Some),
    }
}

/// An encryption that failed, with the vault opened again when the notes
/// stayed unencrypted.
pub struct NotEncrypted {
    /// What went wrong.
    pub error: Error,
    /// The vault and its store as they were, `None` when it is encrypted
    /// after all and has to be unlocked.
    pub open: Option<(Vault, Store)>,
}

/// Encrypts an open, unencrypted vault: its key wrapped with `password`, or
/// kept by `device` (see [`encryption_keys`]).
///
/// # Errors
///
/// What failed, with the notes open as they were when they stayed
/// unencrypted, or an error reading them again.
pub fn encrypt(
    mut vault: Vault,
    store: Store,
    password: Option<&str>,
    device: Option<&dyn KeyStore>,
) -> std::result::Result<(Vault, Store), Box<NotEncrypted>> {
    let encrypted = match device {
        Some(device) => vault.encrypt(store, device),
        None => {
            vault.encrypt_with_password(store, password.unwrap_or_default(), KdfParams::DEFAULT)
        }
    };
    let error = match encrypted {
        Ok(store) => return Ok((vault, store)),
        Err(error) => error,
    };
    // read again from its files: vault.json says whether it got encrypted
    let reopened = vault.reopen().and_then(|vault| {
        if vault.is_encrypted() {
            return Ok(None);
        }
        let store = vault.unlock_plain()?;
        Ok(Some((vault, store)))
    });
    Err(Box::new(match reopened {
        Ok(open) => NotEncrypted { error, open },
        Err(error) => NotEncrypted { error, open: None },
    }))
}

/// Whether `password` opens these notes, nothing else done.
pub fn check_password(vault: &Vault, password: &str) -> bool {
    !password.is_empty() && vault.key_with_password(password).is_ok()
}

/// Makes a new recovery key for the vault, replacing any earlier one, and
/// returns it to show once. `password` when the vault has one.
///
/// # Errors
///
/// [`Error::Invalid`] when no key is at hand, key and file errors.
pub fn create_recovery(
    vault: &mut Vault,
    password: Option<&str>,
    keys: &Keys<'_>,
) -> Result<String> {
    let key = current_key(vault, password, keys)?;
    Ok(vault
        .create_recovery_key_for(&key, KdfParams::DEFAULT)?
        .to_string())
}

/// Removes the recovery key.
///
/// # Errors
///
/// As [`create_recovery`].
pub fn remove_recovery(vault: &mut Vault, password: Option<&str>, keys: &Keys<'_>) -> Result<()> {
    let key = current_key(vault, password, keys)?;
    vault.remove_recovery_key_for(&key)
}

/// The password is forgotten, or an old vault's device key is gone: opens
/// with the recovery key and sets a new password, the only way from then on
/// until a quicker one is chosen again.
///
/// # Errors
///
/// [`Error::Invalid`] without a new password, a wrong recovery key, key and
/// file errors.
pub fn recover(
    vault: &mut Vault,
    code: &str,
    new_password: &str,
    keys: &Keys<'_>,
) -> Result<Store> {
    if new_password.is_empty() {
        return Err(needs_password());
    }
    let key = vault.key_with_recovery(code)?;
    vault.set_password(&key, new_password, KdfParams::DEFAULT)?;
    forget_kept(vault, keys)?;
    vault.unlock_with_key(&key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disk::NativeDisk;
    use crate::keystore::MemoryKeyStore;
    use crate::store::NewNote;

    fn native() -> Arc<dyn Disk> {
        Arc::new(NativeDisk)
    }

    #[test]
    fn without_a_key_store_a_vault_opens_with_its_password_only() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Keys::default();
        let (vault, _) = create(
            native(),
            dir.path(),
            Way::Password,
            "cedar posts",
            None,
            &keys,
        )
        .unwrap();
        let status = serde_json::to_value(VaultStatus::of(&vault, false, &keys)).unwrap();
        assert_eq!(status["way"], "password");
        assert_eq!(status["hasPassword"], true);
        assert_eq!(status["deviceAvailable"], false);
        assert!(matches!(
            unlock(&vault, None, &keys),
            Err(Error::Invalid(_))
        ));
        assert!(unlock(&vault, Some("cedar posts"), &keys).is_ok());
    }

    #[test]
    fn a_way_the_device_cannot_keep_is_refused_before_any_vault_is_made() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Keys::default();
        for way in [Way::Pin, Way::Guarded] {
            let made = create(native(), dir.path(), way, "pw", Some("123456"), &keys);
            assert!(matches!(made, Err(Error::Unsupported(_))));
            assert!(!Vault::exists(dir.path()));
        }
    }

    #[test]
    fn a_pin_way_shows_in_the_status_with_its_tries() {
        let dir = tempfile::tempdir().unwrap();
        let device = MemoryKeyStore::default();
        let keys = Keys {
            device: Some(&device),
            guarded: None,
        };
        let (vault, _) =
            create(native(), dir.path(), Way::Pin, "pw", Some("482915"), &keys).unwrap();
        let status = serde_json::to_value(VaultStatus::of(&vault, false, &keys)).unwrap();
        assert_eq!(status["way"], "pin");
        assert_eq!(status["pinTriesLeft"], 5);
        assert!(unlock_with_pin(&vault, "482915", &keys).is_ok());
    }

    #[test]
    fn a_failed_encryption_leaves_the_notes_open_as_they_were() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Keys::default();
        let (vault, mut store) = create(native(), dir.path(), Way::Plain, "", None, &keys).unwrap();
        let note = store
            .create_note(&NewNote {
                title: "Order more cedar posts".into(),
                ..NewNote::default()
            })
            .unwrap();
        let Err(failed) = encrypt(vault, store, Some(""), None) else {
            panic!("an empty password encrypted the vault");
        };
        assert!(matches!(failed.error, Error::Invalid(_)));
        let (vault, store) = failed.open.expect("the notes stay open");
        assert!(!vault.is_encrypted());
        assert_eq!(store.note(note.id).unwrap().title, "Order more cedar posts");
    }

    #[test]
    fn the_recovery_key_sets_a_new_password() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Keys::default();
        let (mut vault, _) =
            create(native(), dir.path(), Way::Password, "old", None, &keys).unwrap();
        let code = create_recovery(&mut vault, Some("old"), &keys).unwrap();
        assert!(matches!(
            recover(&mut vault, &code, "", &keys),
            Err(Error::Invalid(_))
        ));
        recover(&mut vault, &code, "new", &keys).unwrap();
        assert!(check_password(&vault, "new"));
        assert!(!check_password(&vault, "old"));
    }
}
