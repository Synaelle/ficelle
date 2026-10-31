// SPDX-License-Identifier: AGPL-3.0-or-later

//! Secure storage for the database key on this device.
//!
//! Desktop uses the platform keychain (Windows Credential Manager, macOS
//! Keychain, Secret Service on Linux) through [`OsKeyStore`]. Mobile shells
//! provide their own implementation backed by Android Keystore or the iOS
//! Secure Enclave, with biometric unlock (phase 4).

use std::collections::HashMap;
use std::sync::Mutex;

use zeroize::Zeroizing;

use crate::error::{Error, Result};

/// A small secret store keyed by name.
pub trait KeyStore: Send + Sync {
    /// Reads a secret, `None` when nothing is stored under `name`.
    ///
    /// # Errors
    ///
    /// Fails when the platform store is unavailable.
    fn get(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>>;

    /// Stores or replaces a secret.
    ///
    /// # Errors
    ///
    /// Fails when the platform store is unavailable.
    fn set(&self, name: &str, secret: &[u8]) -> Result<()>;

    /// Removes a secret. Removing a missing secret is not an error.
    ///
    /// # Errors
    ///
    /// Fails when the platform store is unavailable.
    fn delete(&self, name: &str) -> Result<()>;

    /// A PIN mixed with a key this device's hardware keeps and never hands
    /// out, so each guess of a PIN needs the device and a copy of its files
    /// is not enough. Stores without such a
    /// key give the PIN back as it is: the OS key stores on desktops are
    /// tied to the person's account already.
    ///
    /// # Errors
    ///
    /// Fails when the hardware key does not answer.
    fn mix_pin(&self, pin: &str) -> Result<Zeroizing<String>> {
        Ok(Zeroizing::new(pin.to_owned()))
    }
}

/// Keeps secrets in memory only. For tests and dev profiles.
#[derive(Debug, Default)]
pub struct MemoryKeyStore(Mutex<HashMap<String, Zeroizing<Vec<u8>>>>);

impl MemoryKeyStore {
    fn entries(&self) -> Result<std::sync::MutexGuard<'_, HashMap<String, Zeroizing<Vec<u8>>>>> {
        self.0
            .lock()
            .map_err(|_| Error::KeyStore("memory store poisoned".into()))
    }
}

impl KeyStore for MemoryKeyStore {
    fn get(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
        Ok(self.entries()?.get(name).cloned())
    }

    fn set(&self, name: &str, secret: &[u8]) -> Result<()> {
        self.entries()?
            .insert(name.to_owned(), Zeroizing::new(secret.to_vec()));
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<()> {
        self.entries()?.remove(name);
        Ok(())
    }
}

/// The desktop platform keychain. Entries are filed under the app identifier,
/// so dev builds (their own identifier) never touch release secrets.
#[cfg(feature = "os-keyring")]
#[derive(Debug, Default, Clone, Copy)]
pub struct OsKeyStore;

#[cfg(feature = "os-keyring")]
impl OsKeyStore {
    fn entry(name: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(app_brand::IDENTIFIER, name)
            .map_err(|err| Error::KeyStore(err.to_string()))
    }
}

#[cfg(feature = "os-keyring")]
impl KeyStore for OsKeyStore {
    fn get(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
        match Self::entry(name)?.get_secret() {
            Ok(secret) => Ok(Some(Zeroizing::new(secret))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(Error::KeyStore(err.to_string())),
        }
    }

    fn set(&self, name: &str, secret: &[u8]) -> Result<()> {
        Self::entry(name)?
            .set_secret(secret)
            .map_err(|err| Error::KeyStore(err.to_string()))
    }

    fn delete(&self, name: &str) -> Result<()> {
        match Self::entry(name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(Error::KeyStore(err.to_string())),
        }
    }
}

/// Platform user verification that protects a secret: Windows Hello, Touch
/// ID, Android Keystore with biometrics, the Secure Enclave. Each call to
/// [`seal`](Self::seal) or [`open`](Self::open) asks the user.
pub trait Authenticator: Send + Sync {
    /// Whether this device can verify the user at all.
    fn available(&self) -> bool;

    /// Creates or replaces the platform credential for `name` and encrypts
    /// `secret` with it.
    ///
    /// # Errors
    ///
    /// [`Error::Cancelled`] when the user dismisses the prompt, key store
    /// errors otherwise.
    fn seal(&self, name: &str, secret: &[u8]) -> Result<Vec<u8>>;

    /// Decrypts what [`seal`](Self::seal) returned.
    ///
    /// # Errors
    ///
    /// [`Error::Cancelled`] when the user dismisses the prompt,
    /// [`Error::NotRemembered`] when the credential is gone or changed.
    fn open(&self, name: &str, sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>>;

    /// Removes the platform credential. Removing a missing one is fine.
    ///
    /// # Errors
    ///
    /// Key store errors.
    fn remove(&self, name: &str) -> Result<()>;
}

impl<A: Authenticator + ?Sized> Authenticator for &A {
    fn available(&self) -> bool {
        (**self).available()
    }

    fn seal(&self, name: &str, secret: &[u8]) -> Result<Vec<u8>> {
        (**self).seal(name, secret)
    }

    fn open(&self, name: &str, sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        (**self).open(name, sealed)
    }

    fn remove(&self, name: &str) -> Result<()> {
        (**self).remove(name)
    }
}

/// A key store whose secrets only come back after the platform verified the
/// user. `inner` keeps the sealed bytes, which are useless without the
/// authenticator, under their own name so they never replace a plain
/// remembered key.
pub struct GuardedKeyStore<A, K> {
    auth: A,
    inner: K,
}

impl<A: Authenticator, K: KeyStore> GuardedKeyStore<A, K> {
    /// Guards `inner` with `auth`.
    pub const fn new(auth: A, inner: K) -> Self {
        Self { auth, inner }
    }

    fn sealed_name(name: &str) -> String {
        format!("{name}.guarded")
    }

    /// Whether a secret is stored under `name`, without asking the user.
    ///
    /// # Errors
    ///
    /// Key store errors.
    pub fn contains(&self, name: &str) -> Result<bool> {
        Ok(self.inner.get(&Self::sealed_name(name))?.is_some())
    }

    /// Whether the platform can verify the user.
    pub fn available(&self) -> bool {
        self.auth.available()
    }
}

impl<A: Authenticator, K: KeyStore> KeyStore for GuardedKeyStore<A, K> {
    fn get(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
        match self.inner.get(&Self::sealed_name(name))? {
            Some(sealed) => self.auth.open(name, &sealed).map(Some),
            None => Ok(None),
        }
    }

    fn set(&self, name: &str, secret: &[u8]) -> Result<()> {
        let sealed = self.auth.seal(name, secret)?;
        self.inner.set(&Self::sealed_name(name), &sealed)
    }

    fn delete(&self, name: &str) -> Result<()> {
        self.inner.delete(&Self::sealed_name(name))?;
        self.auth.remove(name)
    }
}

impl<K: KeyStore + ?Sized> KeyStore for &K {
    fn get(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
        (**self).get(name)
    }

    fn set(&self, name: &str, secret: &[u8]) -> Result<()> {
        (**self).set(name, secret)
    }

    fn delete(&self, name: &str) -> Result<()> {
        (**self).delete(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Seals by xor, a stand in for real verification.
    #[derive(Default)]
    struct FakeAuth {
        refuse: bool,
    }

    impl Authenticator for FakeAuth {
        fn available(&self) -> bool {
            true
        }
        fn seal(&self, _name: &str, secret: &[u8]) -> Result<Vec<u8>> {
            Ok(secret.iter().map(|b| b ^ 0x5a).collect())
        }
        fn open(&self, _name: &str, sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
            if self.refuse {
                return Err(Error::Cancelled);
            }
            Ok(Zeroizing::new(sealed.iter().map(|b| b ^ 0x5a).collect()))
        }
        fn remove(&self, _name: &str) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn guarded_store_keeps_only_sealed_bytes() {
        let inner = MemoryKeyStore::default();
        let store = GuardedKeyStore::new(FakeAuth::default(), &inner);
        assert!(!store.contains("k").unwrap());
        store.set("k", b"secret").unwrap();
        assert!(store.contains("k").unwrap());
        assert!(inner.get("k").unwrap().is_none(), "plain name stays free");
        let sealed = inner.get("k.guarded").unwrap().unwrap();
        assert_ne!(sealed.as_slice(), b"secret");
        assert_eq!(store.get("k").unwrap().unwrap().as_slice(), b"secret");
        store.delete("k").unwrap();
        assert!(store.get("k").unwrap().is_none());
    }

    #[test]
    fn guarded_store_reports_a_cancelled_prompt() {
        let inner = MemoryKeyStore::default();
        GuardedKeyStore::new(FakeAuth::default(), &inner)
            .set("k", b"secret")
            .unwrap();
        let refusing = GuardedKeyStore::new(FakeAuth { refuse: true }, &inner);
        assert!(matches!(refusing.get("k"), Err(Error::Cancelled)));
    }

    #[test]
    fn memory_store_round_trips_and_deletes() {
        let store = MemoryKeyStore::default();
        assert!(store.get("a").unwrap().is_none());
        store.set("a", b"secret").unwrap();
        assert_eq!(store.get("a").unwrap().unwrap().as_slice(), b"secret");
        store.delete("a").unwrap();
        store.delete("a").unwrap();
        assert!(store.get("a").unwrap().is_none());
    }
}
