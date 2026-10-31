// SPDX-License-Identifier: AGPL-3.0-or-later

//! Key material for the local database.
//!
//! The database is encrypted by SQLCipher with a random 256 bit key. That key
//! is never derived from the password directly, so changing the password only
//! re-wraps the key and never rewrites the database. Wrapping uses Argon2id to
//! turn the password into a key encryption key, then XChaCha20-Poly1305. Only
//! audited RustCrypto implementations are used, no custom primitives.

use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::error::{Error, Result};

const KEY_LEN: usize = 32;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;

/// Random bytes from the operating system generator.
pub(crate) fn random_bytes<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(|_| Error::Crypto)?;
    Ok(bytes)
}

/// The key SQLCipher uses. Wiped from memory when dropped.
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct DatabaseKey([u8; KEY_LEN]);

impl DatabaseKey {
    /// A fresh random key.
    ///
    /// # Errors
    ///
    /// Fails only if the system random generator is unavailable.
    pub fn generate() -> Result<Self> {
        random_bytes().map(Self)
    }

    /// Rebuilds a key from bytes read back from secure storage.
    ///
    /// # Errors
    ///
    /// Fails if the length is not 32 bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let array: [u8; KEY_LEN] = bytes
            .try_into()
            .map_err(|_| Error::Corrupt("database key has the wrong length".into()))?;
        Ok(Self(array))
    }

    /// Raw bytes, for secure storage.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The `PRAGMA key` value. A raw hex key tells SQLCipher to skip its own
    /// key derivation, which would only add a second slow step.
    pub(crate) fn pragma_value(&self) -> Zeroizing<String> {
        let mut text = Zeroizing::new(String::with_capacity(KEY_LEN * 2 + 3));
        text.push_str("x'");
        for byte in &self.0 {
            text.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
            text.push(char::from_digit(u32::from(byte & 0xf), 16).unwrap_or('0'));
        }
        text.push('\'');
        text
    }
}

impl std::fmt::Debug for DatabaseKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DatabaseKey(redacted)")
    }
}

/// Argon2id cost settings, stored next to the wrapped key so they can be
/// raised later without breaking existing vaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    /// Memory in KiB.
    pub memory_kib: u32,
    /// Passes over memory.
    pub iterations: u32,
    /// Parallel lanes.
    pub parallelism: u32,
}

impl KdfParams {
    /// 64 MiB, 3 passes, 1 lane. Above the OWASP minimum (19 MiB, 2 passes)
    /// and still well under a second on a mid-range phone.
    pub const DEFAULT: Self = Self {
        memory_kib: 64 * 1024,
        iterations: 3,
        parallelism: 1,
    };

    /// Very cheap settings so tests stay fast. Never use for real data.
    pub const INSECURE_FOR_TESTS: Self = Self {
        memory_kib: 8,
        iterations: 1,
        parallelism: 1,
    };

    /// A 32 byte key from a password and salt, for what is sealed with a
    /// password outside the vault (backups).
    ///
    /// # Errors
    ///
    /// Settings Argon2id refuses.
    pub fn derive_key(self, password: &str, salt: &[u8]) -> Result<Zeroizing<[u8; KEY_LEN]>> {
        self.derive(password, salt)
    }

    fn derive(self, password: &str, salt: &[u8]) -> Result<Zeroizing<[u8; KEY_LEN]>> {
        let params = Params::new(
            self.memory_kib,
            self.iterations,
            self.parallelism,
            Some(KEY_LEN),
        )
        .map_err(|_| Error::Invalid("unsupported key derivation settings".into()))?;
        let mut out = Zeroizing::new([0u8; KEY_LEN]);
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password_into(password.as_bytes(), salt, out.as_mut())
            .map_err(|_| Error::Crypto)?;
        Ok(out)
    }
}

/// The database key encrypted with a password, safe to store in a plain file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WrappedKey {
    /// Always `argon2id-xchacha20poly1305`, kept for future formats.
    pub scheme: String,
    /// Argon2id settings used.
    pub kdf: KdfParams,
    /// Base64 salt.
    pub salt: String,
    /// Base64 nonce.
    pub nonce: String,
    /// Base64 encrypted key with its authentication tag.
    pub ciphertext: String,
}

const SCHEME: &str = "argon2id-xchacha20poly1305";

impl WrappedKey {
    /// Encrypts `key` with `password`.
    ///
    /// # Errors
    ///
    /// Fails if randomness or the cipher fails.
    pub fn wrap(key: &DatabaseKey, password: &str, kdf: KdfParams) -> Result<Self> {
        let salt: [u8; SALT_LEN] = random_bytes()?;
        let nonce: [u8; NONCE_LEN] = random_bytes()?;
        let kek = kdf.derive(password, &salt)?;
        let cipher = XChaCha20Poly1305::new_from_slice(kek.as_ref()).map_err(|_| Error::Crypto)?;
        let ciphertext = cipher
            .encrypt(&XNonce::from(nonce), key.as_bytes())
            .map_err(|_| Error::Crypto)?;
        Ok(Self {
            scheme: SCHEME.into(),
            kdf,
            salt: BASE64.encode(salt),
            nonce: BASE64.encode(nonce),
            ciphertext: BASE64.encode(ciphertext),
        })
    }

    /// Decrypts the key.
    ///
    /// # Errors
    ///
    /// [`Error::WrongPassword`] when the password does not match, which the
    /// authentication tag detects. [`Error::Corrupt`] when the data is malformed.
    pub fn unwrap(&self, password: &str) -> Result<DatabaseKey> {
        if self.scheme != SCHEME {
            return Err(Error::Corrupt(format!(
                "unknown key scheme {}",
                self.scheme
            )));
        }
        let decode = |field: &str| {
            BASE64
                .decode(field)
                .map_err(|_| Error::Corrupt("wrapped key is not base64".into()))
        };
        let salt = decode(&self.salt)?;
        let nonce: [u8; NONCE_LEN] = decode(&self.nonce)?
            .try_into()
            .map_err(|_| Error::Corrupt("nonce has the wrong length".into()))?;
        let ciphertext = decode(&self.ciphertext)?;
        let kek = self.kdf.derive(password, &salt)?;
        let cipher = XChaCha20Poly1305::new_from_slice(kek.as_ref()).map_err(|_| Error::Crypto)?;
        let plain = Zeroizing::new(
            cipher
                .decrypt(&XNonce::from(nonce), ciphertext.as_ref())
                .map_err(|_| Error::WrongPassword)?,
        );
        DatabaseKey::from_bytes(&plain)
    }
}

/// Context for [`material_key`], so authenticator output used elsewhere can
/// never give the same key.
const MATERIAL_CONTEXT: &[u8] = b"ficelle device unlock v1";

/// 256 bit key from authenticator output (a Windows Hello signature for
/// example). That output is already high entropy, so HMAC-SHA256 is enough
/// and the slow password hash is not needed.
fn material_key(material: &[u8]) -> Result<Zeroizing<[u8; KEY_LEN]>> {
    use hmac::{Hmac, Mac};
    let mut mac = <Hmac<sha2::Sha256> as hmac::KeyInit>::new_from_slice(material)
        .map_err(|_| Error::Crypto)?;
    mac.update(MATERIAL_CONTEXT);
    let mut out = Zeroizing::new([0u8; KEY_LEN]);
    out.copy_from_slice(&mac.finalize().into_bytes());
    Ok(out)
}

/// Encrypts `secret` with key material from a platform authenticator.
/// Returns the nonce followed by the ciphertext.
///
/// # Errors
///
/// Fails if randomness or the cipher fails.
pub fn seal_with_material(material: &[u8], secret: &[u8]) -> Result<Vec<u8>> {
    let key = material_key(material)?;
    let nonce: [u8; NONCE_LEN] = random_bytes()?;
    let cipher = XChaCha20Poly1305::new_from_slice(key.as_ref()).map_err(|_| Error::Crypto)?;
    let ciphertext = cipher
        .encrypt(&XNonce::from(nonce), secret)
        .map_err(|_| Error::Crypto)?;
    Ok([nonce.as_slice(), &ciphertext].concat())
}

/// Reverses [`seal_with_material`].
///
/// # Errors
///
/// [`Error::NotRemembered`] when the material differs (the platform
/// credential was replaced or its biometrics changed), so the caller falls
/// back to the password.
pub fn open_with_material(material: &[u8], sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    if sealed.len() < NONCE_LEN {
        return Err(Error::Corrupt("sealed secret is too short".into()));
    }
    let (nonce, ciphertext) = sealed.split_at(NONCE_LEN);
    let nonce: [u8; NONCE_LEN] = nonce.try_into().map_err(|_| Error::Crypto)?;
    let key = material_key(material)?;
    let cipher = XChaCha20Poly1305::new_from_slice(key.as_ref()).map_err(|_| Error::Crypto)?;
    cipher
        .decrypt(&XNonce::from(nonce), ciphertext)
        .map(Zeroizing::new)
        .map_err(|_| Error::NotRemembered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_sealing_needs_the_same_material() {
        let sealed = seal_with_material(b"signature bytes", b"db key").unwrap();
        assert_eq!(
            open_with_material(b"signature bytes", &sealed)
                .unwrap()
                .as_slice(),
            b"db key"
        );
        assert!(matches!(
            open_with_material(b"other signature", &sealed),
            Err(Error::NotRemembered)
        ));
        assert!(matches!(
            open_with_material(b"signature bytes", &sealed[..10]),
            Err(Error::Corrupt(_))
        ));
    }

    #[test]
    fn wrap_then_unwrap_gives_the_same_key() {
        let key = DatabaseKey::generate().unwrap();
        let wrapped =
            WrappedKey::wrap(&key, "correct horse", KdfParams::INSECURE_FOR_TESTS).unwrap();
        assert_eq!(wrapped.unwrap("correct horse").unwrap(), key);
    }

    #[test]
    fn wrong_password_is_reported_as_such() {
        let key = DatabaseKey::generate().unwrap();
        let wrapped =
            WrappedKey::wrap(&key, "correct horse", KdfParams::INSECURE_FOR_TESTS).unwrap();
        assert!(matches!(
            wrapped.unwrap("battery staple"),
            Err(Error::WrongPassword)
        ));
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        let key = DatabaseKey::generate().unwrap();
        let mut wrapped = WrappedKey::wrap(&key, "pw", KdfParams::INSECURE_FOR_TESTS).unwrap();
        let mut bytes = BASE64.decode(&wrapped.ciphertext).unwrap();
        bytes[0] ^= 1;
        wrapped.ciphertext = BASE64.encode(bytes);
        assert!(wrapped.unwrap("pw").is_err());
    }

    #[test]
    fn same_password_wraps_differently_each_time() {
        let key = DatabaseKey::generate().unwrap();
        let a = WrappedKey::wrap(&key, "pw", KdfParams::INSECURE_FOR_TESTS).unwrap();
        let b = WrappedKey::wrap(&key, "pw", KdfParams::INSECURE_FOR_TESTS).unwrap();
        assert_ne!(a.salt, b.salt);
        assert_ne!(a.ciphertext, b.ciphertext);
    }

    #[test]
    fn pragma_value_is_hex_and_debug_hides_the_key() {
        let key = DatabaseKey::from_bytes(&[0xab; 32]).unwrap();
        assert_eq!(
            key.pragma_value().as_str(),
            format!("x'{}'", "ab".repeat(32))
        );
        assert_eq!(format!("{key:?}"), "DatabaseKey(redacted)");
    }
}
