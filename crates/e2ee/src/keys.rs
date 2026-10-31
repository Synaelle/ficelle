// SPDX-License-Identifier: AGPL-3.0-or-later

//! The account master key and its two wrappings, one opened by the
//! password (through the OPAQUE export key), one by the recovery code. The
//! server stores both wrappings and can open neither.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use rand::RngCore;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::recovery;
use crate::suite::{decode, encode};
use crate::{Error, Result};

/// Format of a wrapped key, its first byte.
const WRAP_VERSION: u8 = 1;
const NONCE_LEN: usize = 24;

/// Purposes of derived keys, each one a different HKDF info and AEAD label,
/// so no key or wrapping can stand in for another.
const PASSWORD_KEK: &[u8] = b"notes e2ee v1 password kek";
const RECOVERY_KEK: &[u8] = b"notes e2ee v1 recovery kek";
const RECOVERY_VERIFIER: &[u8] = b"notes e2ee v1 recovery verifier";
const WRAP_PASSWORD: &[u8] = b"notes e2ee v1 master key by password";
const WRAP_RECOVERY: &[u8] = b"notes e2ee v1 master key by recovery code";

/// The random key every other key of the account hangs from: note keys,
/// identity keys. 256 bits.
pub struct MasterKey(Zeroizing<[u8; 32]>);

/// The master key wrapped twice, as the server keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyBundle {
    /// Format of the bundle.
    pub version: u32,
    /// Opened with the password.
    pub by_password: String,
    /// Opened with the recovery code.
    pub by_recovery: String,
}

/// What signing up makes, on the client.
pub struct NewAccount {
    /// The master key, kept by the client.
    pub master: MasterKey,
    /// Sent to the server.
    pub bundle: KeyBundle,
    /// Shown to the person once, never sent anywhere.
    pub recovery_code: Zeroizing<String>,
    /// Sent to the server, which keeps its hash: proves the recovery code
    /// without revealing it or the key it opens.
    pub recovery_verifier: String,
}

/// Helpers over a bundle.
pub struct AccountKeys;

fn derive(ikm: &[u8], info: &[u8]) -> Zeroizing<[u8; 32]> {
    let mut out = Zeroizing::new([0u8; 32]);
    Hkdf::<Sha256>::new(None, ikm)
        .expand(info, out.as_mut())
        .expect("32 bytes is a valid HKDF length");
    out
}

fn wrap(kek: &[u8; 32], secret: &[u8], label: &[u8]) -> String {
    let mut nonce = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);
    let cipher = XChaCha20Poly1305::new_from_slice(kek).expect("a 32 byte key");
    let sealed = cipher
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: secret,
                aad: label,
            },
        )
        .expect("encryption of a short secret does not fail");
    encode(&[&[WRAP_VERSION], nonce.as_slice(), &sealed].concat())
}

fn unwrap(kek: &[u8; 32], wrapped: &str, label: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    let bytes = decode(wrapped, "wrapped key")?;
    if bytes.len() < 1 + NONCE_LEN || bytes[0] != WRAP_VERSION {
        return Err(Error::Malformed("wrapped key"));
    }
    let nonce: [u8; NONCE_LEN] = bytes[1..=NONCE_LEN]
        .try_into()
        .map_err(|_| Error::Malformed("wrapped key"))?;
    let cipher = XChaCha20Poly1305::new_from_slice(kek).expect("a 32 byte key");
    cipher
        .decrypt(
            &XNonce::from(nonce),
            Payload {
                msg: &bytes[1 + NONCE_LEN..],
                aad: label,
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| Error::WrongKey)
}

fn recovery_kek(code: &str) -> Result<Zeroizing<[u8; 32]>> {
    let normal = recovery::normalize(code)?;
    Ok(derive(normal.as_bytes(), RECOVERY_KEK))
}

impl MasterKey {
    /// A fresh random key.
    pub fn generate() -> Self {
        let mut key = Zeroizing::new([0u8; 32]);
        OsRng.fill_bytes(key.as_mut());
        Self(key)
    }

    /// The raw key, for the client's own storage and the keys below it.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// The key as stored by a client.
    ///
    /// # Errors
    ///
    /// [`Error::Malformed`] unless it is 32 bytes.
    pub fn from_slice(bytes: &[u8]) -> Result<Self> {
        let key: [u8; 32] = bytes
            .try_into()
            .map_err(|_| Error::Malformed("master key"))?;
        Ok(Self(Zeroizing::new(key)))
    }
}

/// A new account's keys, from the export key of its first password.
///
/// # Panics
///
/// Never: the recovery code it reads back was just generated.
pub fn new_account(export_key: &[u8]) -> NewAccount {
    let master = MasterKey::generate();
    let recovery_code = recovery::generate();
    let recovery_kek = recovery_kek(&recovery_code).expect("a code just generated reads back");
    let bundle = KeyBundle {
        version: 1,
        by_password: wrap(
            &derive(export_key, PASSWORD_KEK),
            master.as_bytes(),
            WRAP_PASSWORD,
        ),
        by_recovery: wrap(&recovery_kek, master.as_bytes(), WRAP_RECOVERY),
    };
    let recovery_verifier =
        AccountKeys::recovery_verifier(&recovery_code).expect("a code just generated reads back");
    NewAccount {
        master,
        bundle,
        recovery_code,
        recovery_verifier,
    }
}

impl AccountKeys {
    /// Opens the master key with the export key of a sign-in.
    ///
    /// # Errors
    ///
    /// [`Error::WrongKey`] when the bundle was made for another password.
    pub fn open_with_password(bundle: &KeyBundle, export_key: &[u8]) -> Result<MasterKey> {
        let key = unwrap(
            &derive(export_key, PASSWORD_KEK),
            &bundle.by_password,
            WRAP_PASSWORD,
        )?;
        MasterKey::from_slice(&key)
    }

    /// Opens the master key with the recovery code.
    ///
    /// # Errors
    ///
    /// [`Error::RecoveryCode`] for text that is not a code,
    /// [`Error::WrongKey`] for another account's code.
    pub fn open_with_recovery(bundle: &KeyBundle, code: &str) -> Result<MasterKey> {
        let key = unwrap(&*recovery_kek(code)?, &bundle.by_recovery, WRAP_RECOVERY)?;
        MasterKey::from_slice(&key)
    }

    /// The bundle after a new password, with the export key of its
    /// registration. The recovery wrapping stays.
    pub fn rewrap_password(bundle: &KeyBundle, master: &MasterKey, export_key: &[u8]) -> KeyBundle {
        KeyBundle {
            by_password: wrap(
                &derive(export_key, PASSWORD_KEK),
                master.as_bytes(),
                WRAP_PASSWORD,
            ),
            ..bundle.clone()
        }
    }

    /// What the server keeps the hash of, to know a recovery code is right
    /// before handing out the wrapping it opens.
    ///
    /// # Errors
    ///
    /// [`Error::RecoveryCode`] for text that is not a code.
    pub fn recovery_verifier(code: &str) -> Result<String> {
        let normal = recovery::normalize(code)?;
        Ok(encode(
            derive(normal.as_bytes(), RECOVERY_VERIFIER).as_slice(),
        ))
    }
}
