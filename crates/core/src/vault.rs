// SPDX-License-Identifier: AGPL-3.0-or-later

//! A vault is the folder holding one local database and its key metadata.
//!
//! Layout:
//! - `vault.json`: format version, vault id, and the database key wrapped by
//!   the password (and by the recovery key, when there is one). Contains
//!   nothing secret.
//! - `notes.db`: the SQLite database, encrypted by SQLCipher.
//!
//! A new vault is encrypted with a random key wrapped by a password, which
//! always opens it. Day to day a quicker way can stand in front of it: a
//! copy of the key in a store guarded by Windows Hello or a fingerprint, or
//! wrapped by a PIN in the device's key store with a few tries
//! ([`Vault::set_pin`]). Losing either leaves the password. Vaults from
//! before kept the key in the device's store alone (`deviceOnly`), with no
//! password; older ones may be unencrypted ([`Vault::encrypt`]). Both are
//! given a password when opened.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::blobs::BlobStore;
use crate::crypto::{DatabaseKey, KdfParams, WrappedKey};
use crate::db;
use crate::error::{Error, Result};
use crate::id::VaultId;
use crate::keystore::KeyStore;
use crate::recovery;
use crate::store::Store;

const META_FILE: &str = "vault.json";
const DB_FILE: &str = "notes.db";
const BLOBS_DIR: &str = "blobs";
const FORMAT: u32 = 1;
/// Written by [`Vault::encrypt`] beside the plain ones, then swapped in.
const ENC_DB: &str = "notes.db.encrypting";
const ENC_BLOBS: &str = "blobs.encrypting";

/// Choice made at first launch.
#[derive(Clone, Copy)]
pub enum Encryption<'a> {
    /// Plain SQLite file. No longer offered, kept for tests and old vaults.
    Off,
    /// Encrypted database unlocked with this password.
    On {
        /// The app password.
        password: &'a str,
        /// Argon2id cost, [`KdfParams::DEFAULT`] outside tests.
        kdf: KdfParams,
    },
    /// Encrypted with a random key kept in `keys` alone: no password.
    Device {
        /// Where this device keeps the key.
        keys: &'a dyn KeyStore,
    },
}

impl std::fmt::Debug for Encryption<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Off => "Off",
            Self::On { .. } => "On",
            Self::Device { .. } => "Device",
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Meta {
    format: u32,
    id: VaultId,
    wrapped_key: Option<WrappedKey>,
    /// The same key wrapped by the recovery key, absent in older files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    recovery_key: Option<WrappedKey>,
    /// Encrypted, its key kept by the device alone: no password wraps it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    device_only: bool,
}

/// Digits in a PIN.
pub const PIN_LENGTH: usize = 6;
/// Wrong PINs before the PIN is removed and the password is needed.
pub const PIN_TRIES: u8 = 5;

/// A PIN kept in a device key store: the key wrapped by the PIN, and how
/// many more wrong PINs it takes.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PinEntry {
    wrapped: WrappedKey,
    tries_left: u8,
    /// Wrapped by the PIN through [`KeyStore::mix_pin`]; PINs set before
    /// 0.3.0 were not, and are wrapped again at their next right try.
    #[serde(default)]
    mixed: bool,
}

/// An existing vault, not yet unlocked.
#[derive(Debug)]
pub struct Vault {
    dir: PathBuf,
    meta: Meta,
}

impl Vault {
    /// Tells whether `dir` already holds a vault.
    pub fn exists(dir: &Path) -> bool {
        dir.join(META_FILE).is_file()
    }

    /// Creates a vault in `dir` (created if missing) and returns it unlocked.
    ///
    /// # Errors
    ///
    /// Fails if a vault already exists there, or on file and database errors.
    pub fn create(dir: &Path, encryption: Encryption<'_>) -> Result<(Self, Store)> {
        if Self::exists(dir) {
            return Err(Error::Invalid(
                "a vault already exists in this folder".into(),
            ));
        }
        // a database without metadata is left from another vault, adopting
        // it would mix that vault's notes (and encryption) into this one
        if dir.join(DB_FILE).exists() {
            return Err(Error::Invalid(
                "this folder holds a database from another vault".into(),
            ));
        }
        std::fs::create_dir_all(dir)?;
        let mut keep_in = None;
        let (key, wrapped_key) = match encryption {
            Encryption::Off => (None, None),
            Encryption::On { password, kdf } => {
                if password.is_empty() {
                    return Err(Error::Invalid("an encrypted vault needs a password".into()));
                }
                let key = DatabaseKey::generate()?;
                let wrapped = WrappedKey::wrap(&key, password, kdf)?;
                (Some(key), Some(wrapped))
            }
            Encryption::Device { keys } => {
                keep_in = Some(keys);
                (Some(DatabaseKey::generate()?), None)
            }
        };
        let vault = Self {
            dir: dir.to_owned(),
            meta: Meta {
                format: FORMAT,
                id: VaultId::new(),
                wrapped_key,
                recovery_key: None,
                device_only: keep_in.is_some(),
            },
        };
        // kept before anything is written that needs it
        if let (Some(keys), Some(key)) = (keep_in, &key) {
            keys.set(&vault.device_key_name(), key.as_bytes())?;
        }
        let conn = db::open(&vault.db_path(), key.as_ref())?;
        // metadata last: a crash before this point leaves no half made vault
        vault.write_meta()?;
        Ok((vault, Store::new(conn)))
    }

    /// Reads the vault in `dir`.
    ///
    /// # Errors
    ///
    /// Fails if there is no vault or its metadata is unreadable.
    pub fn open(dir: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(dir.join(META_FILE))?;
        let meta: Meta =
            serde_json::from_str(&text).map_err(|err| Error::Corrupt(err.to_string()))?;
        if meta.format > FORMAT {
            return Err(Error::Invalid(
                "this vault was made by a newer version of the app".into(),
            ));
        }
        let vault = Self {
            dir: dir.to_owned(),
            meta,
        };
        vault.settle_encrypting()?;
        Ok(vault)
    }

    /// Vault id.
    pub fn id(&self) -> VaultId {
        self.meta.id
    }

    /// Whether the database is encrypted.
    pub fn is_encrypted(&self) -> bool {
        self.meta.wrapped_key.is_some() || self.meta.device_only
    }

    /// Whether a password opens it: the strict way. Without one an
    /// encrypted vault opens with the key this device keeps.
    pub fn has_password(&self) -> bool {
        self.meta.wrapped_key.is_some()
    }

    /// Path of the database file.
    pub fn db_path(&self) -> PathBuf {
        self.dir.join(DB_FILE)
    }

    /// Attachment file storage of this vault, encrypted when the vault is.
    /// Needs the unlocked store because the file key lives in the database.
    ///
    /// # Errors
    ///
    /// Database or randomness errors.
    pub fn blobs(&self, store: &mut Store) -> Result<BlobStore> {
        let key = if self.is_encrypted() {
            Some(store.blob_key()?)
        } else {
            None
        };
        Ok(BlobStore::new(self.dir.join(BLOBS_DIR), key))
    }

    /// Opens an unencrypted vault.
    ///
    /// # Errors
    ///
    /// Fails if the vault is encrypted.
    pub fn unlock_plain(&self) -> Result<Store> {
        if self.is_encrypted() {
            return Err(Error::Invalid(
                "this vault is encrypted, a password is needed".into(),
            ));
        }
        Ok(Store::new(db::open(&self.db_path(), None)?))
    }

    /// Opens an encrypted vault with its password.
    ///
    /// # Errors
    ///
    /// [`Error::WrongPassword`] when the password does not match.
    pub fn unlock_with_password(&self, password: &str) -> Result<Store> {
        let key = self.key_from_password(password)?;
        Ok(Store::new(db::open(&self.db_path(), Some(&key))?))
    }

    /// Opens an encrypted vault with the key remembered on this device.
    ///
    /// # Errors
    ///
    /// [`Error::NotRemembered`] when this device has no key for the vault.
    pub fn unlock_with_device(&self, keys: &dyn KeyStore) -> Result<Store> {
        let bytes = keys
            .get(&self.device_key_name())?
            .ok_or(Error::NotRemembered)?;
        let key = DatabaseKey::from_bytes(&bytes)?;
        match db::open(&self.db_path(), Some(&key)) {
            Err(Error::WrongKey) => Err(Error::NotRemembered),
            other => Ok(Store::new(other?)),
        }
    }

    /// The database key, opened with the password.
    ///
    /// # Errors
    ///
    /// [`Error::WrongPassword`], or [`Error::Invalid`] without a password.
    pub fn key_with_password(&self, password: &str) -> Result<DatabaseKey> {
        self.key_from_password(password)
    }

    /// The database key, as `keys` keeps it for this vault.
    ///
    /// # Errors
    ///
    /// [`Error::NotRemembered`] when `keys` has none, or one that no longer
    /// opens the database.
    pub fn key_from(&self, keys: &dyn KeyStore) -> Result<DatabaseKey> {
        let bytes = keys
            .get(&self.device_key_name())?
            .ok_or(Error::NotRemembered)?;
        let key = DatabaseKey::from_bytes(&bytes)?;
        match self.check(&key) {
            Err(Error::WrongKey) => Err(Error::NotRemembered),
            other => other.map(|()| key),
        }
    }

    /// The database key, opened with the recovery code.
    ///
    /// # Errors
    ///
    /// [`Error::WrongPassword`] when the code does not match,
    /// [`Error::Invalid`] when there is none or the text is not a code.
    pub fn key_with_recovery(&self, code: &str) -> Result<DatabaseKey> {
        self.meta
            .recovery_key
            .as_ref()
            .ok_or_else(|| Error::Invalid("this vault has no recovery key".into()))?
            .unwrap(&recovery::normalize(code)?)
    }

    /// Fails unless `key` opens the database.
    fn check(&self, key: &DatabaseKey) -> Result<()> {
        db::open(&self.db_path(), Some(key)).map(drop)
    }

    /// Opens with the key in hand, however it was had.
    ///
    /// # Errors
    ///
    /// [`Error::WrongKey`] or database errors.
    pub fn unlock_with_key(&self, key: &DatabaseKey) -> Result<Store> {
        Ok(Store::new(db::open(&self.db_path(), Some(key))?))
    }

    /// Keeps `key` in `keys` for this vault, after checking it opens it.
    ///
    /// # Errors
    ///
    /// [`Error::WrongKey`], or key store errors.
    pub fn keep_key(&self, key: &DatabaseKey, keys: &dyn KeyStore) -> Result<()> {
        self.check(key)?;
        keys.set(&self.device_key_name(), key.as_bytes())
    }

    /// Adds a password, or replaces it: the strict way once the caller has
    /// forgotten the device's copies of the key.
    ///
    /// # Errors
    ///
    /// [`Error::WrongKey`], an empty password, or file errors.
    pub fn set_password(
        &mut self,
        key: &DatabaseKey,
        password: &str,
        kdf: KdfParams,
    ) -> Result<()> {
        if password.is_empty() {
            return Err(Error::Invalid("the new password is empty".into()));
        }
        self.check(key)?;
        self.meta.wrapped_key = Some(WrappedKey::wrap(key, password, kdf)?);
        self.meta.device_only = false;
        self.write_meta()
    }

    /// Removes the password: the vault then opens with the key the device
    /// keeps, which the caller stored first ([`Vault::keep_key`]).
    ///
    /// # Errors
    ///
    /// [`Error::WrongKey`], or file errors.
    pub fn remove_password(&mut self, key: &DatabaseKey) -> Result<()> {
        self.check(key)?;
        self.meta.wrapped_key = None;
        self.meta.device_only = true;
        self.write_meta()
    }

    /// Encrypts an unencrypted vault with a new key kept in `keys`: the
    /// database and the files are written again, encrypted, beside the plain
    /// ones, then put in their place. A crash on the way is settled at the
    /// next [`Vault::open`]: before `vault.json` says encrypted the plain
    /// vault stays, after it the encrypted one takes over.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] when already encrypted, file, database or key
    /// store errors.
    pub fn encrypt(&mut self, store: Store, keys: &dyn KeyStore) -> Result<Store> {
        if self.is_encrypted() {
            return Err(Error::Invalid("this vault is encrypted already".into()));
        }
        let key = DatabaseKey::generate()?;
        let enc = self.dir.join(ENC_DB);
        let enc_blobs = self.dir.join(ENC_BLOBS);
        remove_if_there(&enc)?;
        if enc_blobs.is_dir() {
            fs::remove_dir_all(&enc_blobs)?;
        }
        let conn = store.conn();
        conn.execute(
            "ATTACH DATABASE ?1 AS encrypted KEY ?2",
            rusqlite::params![enc.to_string_lossy(), key.pragma_value().as_str()],
        )?;
        conn.query_row("SELECT sqlcipher_export('encrypted')", [], |_| Ok(()))?;
        // the export copies the tables, not the schema version migrations read
        let version: i64 = conn.query_row("PRAGMA main.user_version", [], |row| row.get(0))?;
        conn.pragma_update(Some("encrypted"), "user_version", version)?;
        conn.execute("DETACH DATABASE encrypted", [])?;
        drop(store);
        let plain = BlobStore::new(self.dir.join(BLOBS_DIR), None);
        {
            let mut encrypted = Store::new(db::open(&enc, Some(&key))?);
            let files = BlobStore::new(enc_blobs, Some(encrypted.blob_key()?));
            for sha in plain.names()? {
                if let Some(mut reader) = plain.open(&sha)? {
                    files.put_known(&sha, &mut reader)?;
                }
            }
        }
        // kept before vault.json says encrypted: from then on it is needed
        keys.set(&self.device_key_name(), key.as_bytes())?;
        self.meta.device_only = true;
        self.write_meta()?;
        self.settle_encrypting()?;
        self.unlock_with_key(&key)
    }

    /// Finishes or undoes an [`Vault::encrypt`] a crash cut short.
    fn settle_encrypting(&self) -> Result<()> {
        let enc = self.dir.join(ENC_DB);
        let enc_blobs = self.dir.join(ENC_BLOBS);
        if !enc.exists() && !enc_blobs.exists() {
            return Ok(());
        }
        if !self.is_encrypted() {
            // vault.json never said so: the plain vault is the real one
            remove_if_there(&enc)?;
            if enc_blobs.is_dir() {
                fs::remove_dir_all(&enc_blobs)?;
            }
            return Ok(());
        }
        if enc.exists() {
            for name in [DB_FILE, "notes.db-wal", "notes.db-shm", "notes.db-journal"] {
                remove_if_there(&self.dir.join(name))?;
            }
            fs::rename(&enc, self.db_path())?;
        }
        if enc_blobs.is_dir() {
            let blobs = self.dir.join(BLOBS_DIR);
            if blobs.is_dir() {
                fs::remove_dir_all(&blobs)?;
            }
            fs::rename(&enc_blobs, blobs)?;
        }
        Ok(())
    }

    /// Stores the key in the device key store so the next unlock needs no
    /// password. Asks for the password to prove the right to do so.
    ///
    /// # Errors
    ///
    /// [`Error::WrongPassword`] or key store errors.
    pub fn remember_on_device(&self, password: &str, keys: &dyn KeyStore) -> Result<()> {
        let key = self.key_from_password(password)?;
        keys.set(&self.device_key_name(), key.as_bytes())
    }

    /// Removes the remembered key from this device.
    ///
    /// # Errors
    ///
    /// Key store errors.
    pub fn forget_on_device(&self, keys: &dyn KeyStore) -> Result<()> {
        keys.delete(&self.device_key_name())
    }

    /// Changes the password. Only the small wrapped key is rewritten, the
    /// database stays as it is.
    ///
    /// # Errors
    ///
    /// [`Error::WrongPassword`] when `old` does not match.
    pub fn change_password(&mut self, old: &str, new: &str, kdf: KdfParams) -> Result<()> {
        if new.is_empty() {
            return Err(Error::Invalid("the new password is empty".into()));
        }
        let key = self.key_from_password(old)?;
        self.meta.wrapped_key = Some(WrappedKey::wrap(&key, new, kdf)?);
        self.write_meta()
    }

    /// Whether a recovery key can open this vault.
    pub fn has_recovery_key(&self) -> bool {
        self.meta.recovery_key.is_some()
    }

    /// Makes a new recovery key, replacing any earlier one, and returns it to
    /// show once. Asks for the password to prove the right to do so.
    ///
    /// # Errors
    ///
    /// [`Error::WrongPassword`], or file errors.
    pub fn create_recovery_key(
        &mut self,
        password: &str,
        kdf: KdfParams,
    ) -> Result<zeroize::Zeroizing<String>> {
        let key = self.key_from_password(password)?;
        self.create_recovery_key_for(&key, kdf)
    }

    /// The same with the key in hand, for a vault without a password.
    ///
    /// # Errors
    ///
    /// [`Error::WrongKey`], or file errors.
    pub fn create_recovery_key_for(
        &mut self,
        key: &DatabaseKey,
        kdf: KdfParams,
    ) -> Result<zeroize::Zeroizing<String>> {
        self.check(key)?;
        let code = recovery::generate()?;
        let wrapped = WrappedKey::wrap(key, &recovery::normalize(&code)?, kdf)?;
        self.meta.recovery_key = Some(wrapped);
        self.write_meta()?;
        Ok(code)
    }

    /// Removes the recovery key, the key in hand proving the right to.
    ///
    /// # Errors
    ///
    /// [`Error::WrongKey`], or file errors.
    pub fn remove_recovery_key_for(&mut self, key: &DatabaseKey) -> Result<()> {
        self.check(key)?;
        self.meta.recovery_key = None;
        self.write_meta()
    }

    /// Removes the recovery key, after checking the password.
    ///
    /// # Errors
    ///
    /// [`Error::WrongPassword`], or file errors.
    pub fn remove_recovery_key(&mut self, password: &str) -> Result<()> {
        self.key_from_password(password)?;
        self.meta.recovery_key = None;
        self.write_meta()
    }

    /// Opens the vault with the recovery key and sets a new password. The
    /// recovery key keeps working afterwards.
    ///
    /// # Errors
    ///
    /// [`Error::WrongPassword`] when the recovery key does not match,
    /// [`Error::Invalid`] when there is none or the text is not a key.
    pub fn recover(&mut self, code: &str, new_password: &str, kdf: KdfParams) -> Result<Store> {
        if new_password.is_empty() {
            return Err(Error::Invalid("the new password is empty".into()));
        }
        let key = self
            .meta
            .recovery_key
            .as_ref()
            .ok_or_else(|| Error::Invalid("this vault has no recovery key".into()))?
            .unwrap(&recovery::normalize(code)?)?;
        let store = Store::new(db::open(&self.db_path(), Some(&key))?);
        self.meta.wrapped_key = Some(WrappedKey::wrap(&key, new_password, kdf)?);
        self.write_meta()?;
        Ok(store)
    }

    fn key_from_password(&self, password: &str) -> Result<DatabaseKey> {
        self.meta
            .wrapped_key
            .as_ref()
            .ok_or_else(|| Error::Invalid("this vault is not encrypted".into()))?
            .unwrap(password)
    }

    /// Deletes the vault in `dir` for good: its database (with SQLite's
    /// side files), its settings, its files and the imported themes beside
    /// them. Only those names: anything else the person put in the folder
    /// stays, and the folder with it. Nothing when there is no vault.
    ///
    /// # Errors
    ///
    /// When a file cannot be removed, the ones before it are gone.
    pub fn destroy(dir: &Path) -> Result<()> {
        // the database first: once it is gone there is no vault to open
        for name in [DB_FILE, "notes.db-wal", "notes.db-shm", "notes.db-journal"] {
            remove_if_there(&dir.join(name))?;
        }
        for name in [BLOBS_DIR, "themes"] {
            let path = dir.join(name);
            if path.is_dir() {
                fs::remove_dir_all(&path)?;
            }
        }
        remove_if_there(&dir.join(format!("{META_FILE}.tmp")))?;
        remove_if_there(&dir.join(META_FILE))?;
        // an empty folder goes too, one with other things in it stays
        if dir.is_dir() && fs::read_dir(dir)?.next().is_none() {
            fs::remove_dir(dir)?;
        }
        Ok(())
    }

    /// Keeps a PIN to open the vault on this device: the key wrapped by the
    /// PIN, in `keys` and never beside the database, with [`PIN_TRIES`]
    /// tries. Replaces an earlier PIN.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] for a PIN that is not [`PIN_LENGTH`] digits,
    /// [`Error::WrongKey`], or key store errors.
    pub fn set_pin(
        &self,
        key: &DatabaseKey,
        pin: &str,
        keys: &dyn KeyStore,
        kdf: KdfParams,
    ) -> Result<()> {
        if pin.len() != PIN_LENGTH || !pin.bytes().all(|b| b.is_ascii_digit()) {
            return Err(Error::Invalid(format!("a PIN is {PIN_LENGTH} digits")));
        }
        self.check(key)?;
        let entry = PinEntry {
            wrapped: WrappedKey::wrap(key, &keys.mix_pin(pin)?, kdf)?,
            tries_left: PIN_TRIES,
            mixed: true,
        };
        self.write_pin(&entry, keys)
    }

    /// Whether this device keeps a PIN for the vault, and the tries left.
    ///
    /// # Errors
    ///
    /// Key store errors.
    pub fn pin_tries_left(&self, keys: &dyn KeyStore) -> Result<Option<u8>> {
        Ok(self.read_pin(keys)?.map(|entry| entry.tries_left))
    }

    /// The database key, opened with the PIN. A wrong PIN uses a try, the
    /// last one removes the PIN: the password is needed then.
    ///
    /// # Errors
    ///
    /// [`Error::WrongPassword`] for a wrong PIN with tries left,
    /// [`Error::NotRemembered`] when there is no PIN (any more).
    pub fn key_with_pin(&self, pin: &str, keys: &dyn KeyStore) -> Result<DatabaseKey> {
        let mut entry = self.read_pin(keys)?.ok_or(Error::NotRemembered)?;
        // the try is counted before the check, so stopping the app in the
        // middle of one does not give it back
        entry.tries_left = entry.tries_left.saturating_sub(1);
        if entry.tries_left == 0 {
            keys.delete(&self.pin_name())?;
        } else {
            self.write_pin(&entry, keys)?;
        }
        let secret = if entry.mixed {
            keys.mix_pin(pin)?
        } else {
            zeroize::Zeroizing::new(pin.to_owned())
        };
        match entry.wrapped.unwrap(&secret) {
            Ok(key) => {
                self.check(&key)?;
                entry.tries_left = PIN_TRIES;
                if !entry.mixed {
                    entry.wrapped = WrappedKey::wrap(&key, &keys.mix_pin(pin)?, entry.wrapped.kdf)?;
                    entry.mixed = true;
                }
                self.write_pin(&entry, keys)?;
                Ok(key)
            }
            Err(Error::WrongPassword) if entry.tries_left == 0 => Err(Error::NotRemembered),
            Err(err) => Err(err),
        }
    }

    /// Removes the PIN from `keys`.
    ///
    /// # Errors
    ///
    /// Key store errors.
    pub fn remove_pin(&self, keys: &dyn KeyStore) -> Result<()> {
        keys.delete(&self.pin_name())
    }

    fn pin_name(&self) -> String {
        format!("{}.pin", self.device_key_name())
    }

    fn read_pin(&self, keys: &dyn KeyStore) -> Result<Option<PinEntry>> {
        keys.get(&self.pin_name())?
            .map(|bytes| {
                serde_json::from_slice(&bytes).map_err(|err| Error::Corrupt(err.to_string()))
            })
            .transpose()
    }

    fn write_pin(&self, entry: &PinEntry, keys: &dyn KeyStore) -> Result<()> {
        let bytes = serde_json::to_vec(entry).map_err(|err| Error::Corrupt(err.to_string()))?;
        keys.set(&self.pin_name(), &bytes)
    }

    /// Name of this vault's entry in a device key store.
    pub fn device_key_name(&self) -> String {
        format!("vault-{}", self.meta.id)
    }

    fn write_meta(&self) -> Result<()> {
        let text = serde_json::to_string_pretty(&self.meta)
            .map_err(|err| Error::Corrupt(err.to_string()))?;
        // write then rename so a crash never leaves a truncated file
        let tmp = self.dir.join(format!("{META_FILE}.tmp"));
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, self.dir.join(META_FILE))?;
        Ok(())
    }
}

fn remove_if_there(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keystore::MemoryKeyStore;
    use crate::store::NewNote;

    const KDF: KdfParams = KdfParams::INSECURE_FOR_TESTS;

    fn on(password: &str) -> Encryption<'_> {
        Encryption::On { password, kdf: KDF }
    }

    #[test]
    fn a_pin_opens_with_five_tries_then_needs_the_password() {
        let dir = tempfile::tempdir().unwrap();
        let keys = MemoryKeyStore::default();
        let (vault, store) = Vault::create(dir.path(), on("pw")).unwrap();
        drop(store);
        let key = vault.key_with_password("pw").unwrap();
        for bad in ["12345", "1234567", "12345a", ""] {
            assert!(matches!(
                vault.set_pin(&key, bad, &keys, KDF),
                Err(Error::Invalid(_))
            ));
        }
        assert_eq!(vault.pin_tries_left(&keys).unwrap(), None);
        vault.set_pin(&key, "024680", &keys, KDF).unwrap();
        assert_eq!(vault.pin_tries_left(&keys).unwrap(), Some(PIN_TRIES));
        let raw = std::fs::read_to_string(dir.path().join(META_FILE)).unwrap();
        assert!(
            !raw.contains("pin"),
            "nothing of the PIN beside the database"
        );

        // a right PIN gives the tries back
        assert!(matches!(
            vault.key_with_pin("111111", &keys),
            Err(Error::WrongPassword)
        ));
        assert_eq!(vault.pin_tries_left(&keys).unwrap(), Some(PIN_TRIES - 1));
        let opened = vault.key_with_pin("024680", &keys).unwrap();
        assert_eq!(opened.as_bytes(), key.as_bytes());
        assert_eq!(vault.pin_tries_left(&keys).unwrap(), Some(PIN_TRIES));

        // five wrong ones remove it, the right one no longer works
        for left in (1..PIN_TRIES).rev() {
            assert!(matches!(
                vault.key_with_pin("999999", &keys),
                Err(Error::WrongPassword)
            ));
            assert_eq!(vault.pin_tries_left(&keys).unwrap(), Some(left));
        }
        assert!(matches!(
            vault.key_with_pin("999999", &keys),
            Err(Error::NotRemembered)
        ));
        assert_eq!(vault.pin_tries_left(&keys).unwrap(), None);
        assert!(matches!(
            vault.key_with_pin("024680", &keys),
            Err(Error::NotRemembered)
        ));
        assert!(vault.unlock_with_password("pw").is_ok());

        // set again, then removed
        vault.set_pin(&key, "135791", &keys, KDF).unwrap();
        vault.remove_pin(&keys).unwrap();
        assert_eq!(vault.pin_tries_left(&keys).unwrap(), None);
    }

    /// A store with a hardware key, the device's alone.
    #[derive(Default)]
    struct Hardware(MemoryKeyStore);

    impl KeyStore for Hardware {
        fn get(&self, name: &str) -> Result<Option<zeroize::Zeroizing<Vec<u8>>>> {
            self.0.get(name)
        }
        fn set(&self, name: &str, secret: &[u8]) -> Result<()> {
            self.0.set(name, secret)
        }
        fn delete(&self, name: &str) -> Result<()> {
            self.0.delete(name)
        }
        fn mix_pin(&self, pin: &str) -> Result<zeroize::Zeroizing<String>> {
            Ok(zeroize::Zeroizing::new(format!("device secret {pin}")))
        }
    }

    #[test]
    fn a_pin_needs_the_device_s_hardware_key_and_old_pins_move_to_it() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Hardware::default();
        let (vault, store) = Vault::create(dir.path(), on("pw")).unwrap();
        drop(store);
        let key = vault.key_with_password("pw").unwrap();

        // a PIN from before 0.3.0, wrapped by the PIN alone
        let old = PinEntry {
            wrapped: WrappedKey::wrap(&key, "024680", KDF).unwrap(),
            tries_left: PIN_TRIES,
            mixed: false,
        };
        vault.write_pin(&old, &keys).unwrap();
        assert!(vault.key_with_pin("024680", &keys).is_ok());
        let entry = vault.read_pin(&keys).unwrap().unwrap();
        assert!(entry.mixed, "wrapped again through the hardware key");
        assert!(entry.wrapped.unwrap("024680").is_err());

        // the files copied off the device do not open with the PIN alone
        let copy = MemoryKeyStore::default();
        copy.set(
            &vault.pin_name(),
            &keys.get(&vault.pin_name()).unwrap().unwrap(),
        )
        .unwrap();
        assert!(matches!(
            vault.key_with_pin("024680", &copy),
            Err(Error::WrongPassword)
        ));
        assert!(vault.key_with_pin("024680", &keys).is_ok());
    }

    #[test]
    fn a_pin_needs_the_vaults_key() {
        let dir = tempfile::tempdir().unwrap();
        let keys = MemoryKeyStore::default();
        let (vault, store) = Vault::create(dir.path(), on("pw")).unwrap();
        drop(store);
        let other = DatabaseKey::generate().unwrap();
        assert!(vault.set_pin(&other, "123456", &keys, KDF).is_err());
        assert_eq!(vault.pin_tries_left(&keys).unwrap(), None);
    }

    #[test]
    fn plain_vault_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let (vault, mut store) = Vault::create(dir.path(), Encryption::Off).unwrap();
        assert!(!vault.is_encrypted());
        store
            .create_note(&NewNote {
                title: "hello".into(),
                ..NewNote::default()
            })
            .unwrap();
        drop(store);
        let store = Vault::open(dir.path()).unwrap().unlock_plain().unwrap();
        assert_eq!(store.search("hello", 5).unwrap().len(), 1);
    }

    #[test]
    fn refuses_a_leftover_database_without_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let (_, store) = Vault::create(dir.path(), Encryption::Off).unwrap();
        drop(store);
        std::fs::remove_file(dir.path().join(META_FILE)).unwrap();
        assert!(matches!(
            Vault::create(dir.path(), on("pw")),
            Err(Error::Invalid(_))
        ));
        assert!(!Vault::exists(dir.path()), "nothing half made is left");
    }

    #[test]
    fn recovery_key_sets_a_new_password() {
        let dir = tempfile::tempdir().unwrap();
        let (mut vault, mut store) = Vault::create(dir.path(), on("old")).unwrap();
        store
            .create_note(&NewNote {
                title: "kept".into(),
                ..NewNote::default()
            })
            .unwrap();
        drop(store);
        assert!(!vault.has_recovery_key());
        assert!(matches!(
            vault.create_recovery_key("wrong", KDF),
            Err(Error::WrongPassword)
        ));
        let code = vault.create_recovery_key("old", KDF).unwrap();

        let mut vault = Vault::open(dir.path()).unwrap();
        assert!(vault.has_recovery_key());
        let other = recovery::generate().unwrap();
        assert!(matches!(
            vault.recover(&other, "new", KDF),
            Err(Error::WrongPassword)
        ));
        let typed = code.to_lowercase().replace('-', " ");
        let store = vault.recover(&typed, "new", KDF).unwrap();
        assert_eq!(store.search("kept", 5).unwrap().len(), 1);
        drop(store);

        let vault = Vault::open(dir.path()).unwrap();
        assert!(matches!(
            vault.unlock_with_password("old"),
            Err(Error::WrongPassword)
        ));
        assert!(vault.unlock_with_password("new").is_ok());
        assert!(vault.has_recovery_key(), "the key keeps working");
    }

    #[test]
    fn recovery_key_survives_a_password_change_and_can_be_removed() {
        let dir = tempfile::tempdir().unwrap();
        let (mut vault, store) = Vault::create(dir.path(), on("a")).unwrap();
        drop(store);
        let code = vault.create_recovery_key("a", KDF).unwrap();
        vault.change_password("a", "b", KDF).unwrap();
        drop(vault.recover(&code, "c", KDF).unwrap());
        vault.remove_recovery_key("c").unwrap();
        assert!(!Vault::open(dir.path()).unwrap().has_recovery_key());
        assert!(matches!(
            vault.recover(&code, "d", KDF),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn encrypted_vault_needs_the_password() {
        let dir = tempfile::tempdir().unwrap();
        let (_, mut store) = Vault::create(dir.path(), on("pw")).unwrap();
        store
            .create_note(&NewNote {
                title: "secret plan".into(),
                ..NewNote::default()
            })
            .unwrap();
        drop(store);

        let raw = std::fs::read(dir.path().join(DB_FILE)).unwrap();
        assert!(!raw.windows(6).any(|w| w == b"secret"));

        let vault = Vault::open(dir.path()).unwrap();
        assert!(vault.is_encrypted());
        assert!(matches!(
            vault.unlock_with_password("nope"),
            Err(Error::WrongPassword)
        ));
        assert!(vault.unlock_plain().is_err());
        assert_eq!(
            vault
                .unlock_with_password("pw")
                .unwrap()
                .search("secret", 5)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn device_unlock_after_remembering() {
        let dir = tempfile::tempdir().unwrap();
        let keys = MemoryKeyStore::default();
        let (vault, _) = Vault::create(dir.path(), on("pw")).unwrap();
        assert!(matches!(
            vault.unlock_with_device(&keys),
            Err(Error::NotRemembered)
        ));
        assert!(vault.remember_on_device("wrong", &keys).is_err());
        vault.remember_on_device("pw", &keys).unwrap();
        vault.unlock_with_device(&keys).unwrap();
        vault.forget_on_device(&keys).unwrap();
        assert!(matches!(
            vault.unlock_with_device(&keys),
            Err(Error::NotRemembered)
        ));
    }

    #[test]
    fn password_change_keeps_the_data() {
        let dir = tempfile::tempdir().unwrap();
        let (mut vault, mut store) = Vault::create(dir.path(), on("old")).unwrap();
        store
            .create_note(&NewNote {
                title: "kept".into(),
                ..NewNote::default()
            })
            .unwrap();
        drop(store);
        assert!(vault.change_password("bad", "new", KDF).is_err());
        vault.change_password("old", "new", KDF).unwrap();

        let vault = Vault::open(dir.path()).unwrap();
        assert!(vault.unlock_with_password("old").is_err());
        assert_eq!(
            vault
                .unlock_with_password("new")
                .unwrap()
                .search("kept", 5)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn blob_files_follow_the_vault_encryption() {
        let dir = tempfile::tempdir().unwrap();
        let (vault, mut store) = Vault::create(dir.path(), on("pw")).unwrap();
        let blobs = vault.blobs(&mut store).unwrap();
        let sha = blobs.put(b"picture").unwrap();
        drop(store);
        let mut store = Vault::open(dir.path())
            .unwrap()
            .unlock_with_password("pw")
            .unwrap();
        assert_eq!(
            vault.blobs(&mut store).unwrap().get(&sha).unwrap().unwrap(),
            b"picture"
        );
    }

    #[test]
    fn destroying_removes_the_vault_and_only_it() {
        let dir = tempfile::tempdir().unwrap();
        let vault_dir = dir.path().join("vault");
        let (_, mut store) = Vault::create(&vault_dir, Encryption::Off).unwrap();
        store
            .create_note(&crate::store::NewNote::default())
            .unwrap();
        drop(store);
        fs::create_dir_all(vault_dir.join(BLOBS_DIR).join("ab")).unwrap();
        fs::write(vault_dir.join(BLOBS_DIR).join("ab").join("file"), "x").unwrap();
        fs::create_dir_all(vault_dir.join("themes")).unwrap();
        fs::write(vault_dir.join("themes").join("mine.json"), "{}").unwrap();
        fs::write(dir.path().join("beside.txt"), "not the vault's").unwrap();

        Vault::destroy(&vault_dir).unwrap();
        assert!(!Vault::exists(&vault_dir));
        assert!(!vault_dir.exists(), "an emptied folder goes too");
        assert!(dir.path().join("beside.txt").exists());
        Vault::destroy(&vault_dir).unwrap();

        // a file the person put in the folder keeps it, and the folder
        let (_, store) = Vault::create(&vault_dir, Encryption::Off).unwrap();
        drop(store);
        fs::write(vault_dir.join("my notes.txt"), "mine").unwrap();
        Vault::destroy(&vault_dir).unwrap();
        assert!(!Vault::exists(&vault_dir));
        assert!(!vault_dir.join(DB_FILE).exists());
        assert!(vault_dir.join("my notes.txt").exists());
        Vault::create(&vault_dir, Encryption::Off).unwrap();
    }

    fn note(store: &mut Store, title: &str) {
        store
            .create_note(&NewNote {
                title: title.into(),
                ..NewNote::default()
            })
            .unwrap();
    }

    #[test]
    fn a_device_kept_vault_opens_without_a_password_and_is_encrypted() {
        let dir = tempfile::tempdir().unwrap();
        let keys = MemoryKeyStore::default();
        let (_, mut store) = Vault::create(dir.path(), Encryption::Device { keys: &keys }).unwrap();
        note(&mut store, "secret plan");
        drop(store);
        let raw = std::fs::read(dir.path().join(DB_FILE)).unwrap();
        assert!(!raw.windows(6).any(|w| w == b"secret"), "encrypted on disk");

        let vault = Vault::open(dir.path()).unwrap();
        assert!(vault.is_encrypted() && !vault.has_password());
        assert!(vault.unlock_plain().is_err());
        assert!(matches!(
            vault.unlock_with_password("anything"),
            Err(Error::Invalid(_))
        ));
        let store = vault.unlock_with_device(&keys).unwrap();
        assert_eq!(store.search("secret", 5).unwrap().len(), 1);
        drop(store);
        // another device's store, or a lost one, opens nothing
        let elsewhere = MemoryKeyStore::default();
        assert!(matches!(
            vault.key_from(&elsewhere),
            Err(Error::NotRemembered)
        ));
        drop(vault);
    }

    #[test]
    fn the_ways_switch_back_and_forth_keeping_the_notes() {
        let dir = tempfile::tempdir().unwrap();
        let keys = MemoryKeyStore::default();
        let (mut vault, mut store) =
            Vault::create(dir.path(), Encryption::Device { keys: &keys }).unwrap();
        note(&mut store, "kept");
        drop(store);

        // the strict way: a password, the device's copy forgotten
        let key = vault.key_from(&keys).unwrap();
        vault.set_password(&key, "pw", KDF).unwrap();
        vault.forget_on_device(&keys).unwrap();
        let vault2 = Vault::open(dir.path()).unwrap();
        assert!(vault2.has_password());
        assert!(matches!(
            vault2.unlock_with_device(&keys),
            Err(Error::NotRemembered)
        ));
        assert_eq!(
            vault2
                .unlock_with_password("pw")
                .unwrap()
                .search("kept", 5)
                .unwrap()
                .len(),
            1
        );

        // and back: kept by the device again, the password gone
        let key = vault.key_with_password("pw").unwrap();
        vault.keep_key(&key, &keys).unwrap();
        vault.remove_password(&key).unwrap();
        let vault3 = Vault::open(dir.path()).unwrap();
        assert!(vault3.is_encrypted() && !vault3.has_password());
        assert_eq!(
            vault3
                .unlock_with_device(&keys)
                .unwrap()
                .search("kept", 5)
                .unwrap()
                .len(),
            1
        );

        // a key of another vault changes nothing
        let other_dir = tempfile::tempdir().unwrap();
        let other_keys = MemoryKeyStore::default();
        let (other, _) =
            Vault::create(other_dir.path(), Encryption::Device { keys: &other_keys }).unwrap();
        let wrong = other.key_from(&other_keys).unwrap();
        assert!(matches!(
            vault.set_password(&wrong, "x", KDF),
            Err(Error::WrongKey)
        ));
        assert!(matches!(
            vault.remove_password(&wrong),
            Err(Error::WrongKey)
        ));
        assert!(matches!(
            vault.keep_key(&wrong, &keys),
            Err(Error::WrongKey)
        ));
    }

    #[test]
    fn a_device_kept_vault_comes_back_with_its_recovery_code() {
        let dir = tempfile::tempdir().unwrap();
        let keys = MemoryKeyStore::default();
        let (mut vault, mut store) =
            Vault::create(dir.path(), Encryption::Device { keys: &keys }).unwrap();
        note(&mut store, "kept");
        drop(store);
        let key = vault.key_from(&keys).unwrap();
        let code = vault.create_recovery_key_for(&key, KDF).unwrap();

        // a new computer: the device's key is gone
        let fresh = MemoryKeyStore::default();
        let vault = Vault::open(dir.path()).unwrap();
        assert!(matches!(
            vault.unlock_with_device(&fresh),
            Err(Error::NotRemembered)
        ));
        let key = vault.key_with_recovery(&code).unwrap();
        vault.keep_key(&key, &fresh).unwrap();
        assert_eq!(
            vault
                .unlock_with_device(&fresh)
                .unwrap()
                .search("kept", 5)
                .unwrap()
                .len(),
            1
        );
        let mut vault = vault;
        vault.remove_recovery_key_for(&key).unwrap();
        assert!(!Vault::open(dir.path()).unwrap().has_recovery_key());
    }

    #[test]
    fn an_old_unencrypted_vault_is_encrypted_with_its_files() {
        let dir = tempfile::tempdir().unwrap();
        let keys = MemoryKeyStore::default();
        let (mut vault, mut store) = Vault::create(dir.path(), Encryption::Off).unwrap();
        note(&mut store, "secret plan");
        let sha = vault
            .blobs(&mut store)
            .unwrap()
            .put(b"secret picture")
            .unwrap();
        assert!(
            dir.path()
                .join(BLOBS_DIR)
                .join(&sha[..2])
                .join(&sha)
                .exists()
        );

        let mut store = vault.encrypt(store, &keys).unwrap();
        assert!(vault.is_encrypted() && !vault.has_password());
        assert_eq!(store.search("secret", 5).unwrap().len(), 1);
        let blobs = vault.blobs(&mut store).unwrap();
        assert_eq!(blobs.get(&sha).unwrap().unwrap(), b"secret picture");
        drop(store);

        // nothing readable is left: the database, the files, their names
        let raw = std::fs::read(dir.path().join(DB_FILE)).unwrap();
        assert!(!raw.windows(6).any(|w| w == b"secret"));
        assert!(
            !dir.path()
                .join(BLOBS_DIR)
                .join(&sha[..2])
                .join(&sha)
                .exists()
        );
        for entry in walk(&dir.path().join(BLOBS_DIR)) {
            let bytes = std::fs::read(&entry).unwrap();
            assert!(
                !bytes.windows(6).any(|w| w == b"secret"),
                "{}",
                entry.display()
            );
        }
        assert!(!dir.path().join(ENC_DB).exists() && !dir.path().join(ENC_BLOBS).exists());
        let vault = Vault::open(dir.path()).unwrap();
        assert_eq!(
            vault
                .unlock_with_device(&keys)
                .unwrap()
                .search("secret", 5)
                .unwrap()
                .len(),
            1
        );
        assert!(matches!(
            Vault::open(dir.path())
                .unwrap()
                .encrypt(vault.unlock_with_device(&keys).unwrap(), &keys),
            Err(Error::Invalid(_))
        ));
    }

    fn walk(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(walk(&path));
            } else {
                out.push(path);
            }
        }
        out
    }

    #[test]
    fn an_encryption_cut_short_is_settled_at_the_next_open() {
        // before vault.json said encrypted: the plain vault stays
        let dir = tempfile::tempdir().unwrap();
        let (_, mut store) = Vault::create(dir.path(), Encryption::Off).unwrap();
        note(&mut store, "plain");
        drop(store);
        std::fs::write(dir.path().join(ENC_DB), b"half written").unwrap();
        std::fs::create_dir_all(dir.path().join(ENC_BLOBS)).unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        assert!(!dir.path().join(ENC_DB).exists() && !dir.path().join(ENC_BLOBS).exists());
        assert_eq!(
            vault
                .unlock_plain()
                .unwrap()
                .search("plain", 5)
                .unwrap()
                .len(),
            1
        );

        // after it: the encrypted copy takes over
        let keys = MemoryKeyStore::default();
        let plain_dir = tempfile::tempdir().unwrap();
        let (mut vault, mut store) = Vault::create(plain_dir.path(), Encryption::Off).unwrap();
        note(&mut store, "moved");
        let encrypted = vault.encrypt(store, &keys).unwrap();
        drop(encrypted);
        // put the state back to "written, vault.json updated, not swapped"
        let other = tempfile::tempdir().unwrap();
        let (_, mut plain) = Vault::create(other.path(), Encryption::Off).unwrap();
        note(&mut plain, "stale plain");
        drop(plain);
        std::fs::rename(
            plain_dir.path().join(DB_FILE),
            plain_dir.path().join(ENC_DB),
        )
        .unwrap();
        std::fs::copy(other.path().join(DB_FILE), plain_dir.path().join(DB_FILE)).unwrap();
        let vault = Vault::open(plain_dir.path()).unwrap();
        assert!(!plain_dir.path().join(ENC_DB).exists());
        assert_eq!(
            vault
                .unlock_with_device(&keys)
                .unwrap()
                .search("moved", 5)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn refuses_to_create_twice_or_without_password() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Vault::create(dir.path(), on("")).is_err());
        Vault::create(dir.path(), Encryption::Off).unwrap();
        assert!(Vault::create(dir.path(), Encryption::Off).is_err());
    }
}
