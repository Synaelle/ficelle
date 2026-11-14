// SPDX-License-Identifier: AGPL-3.0-or-later

//! Handing the password's export key from the server's sign-in page to the
//! app that opened it. The app makes a key pair for this sign-in only and
//! puts its public half in the address it opens; the page seals the export
//! key to it with HPKE (RFC 9180, base mode, the same suite as sharing) and
//! the info binds it to the sign-in's PKCE challenge. The server passes the
//! sealed key along once and cannot open it, neither can another sign-in.

use hpke::aead::ChaCha20Poly1305;
use hpke::kdf::HkdfSha256;
use hpke::kem::X25519HkdfSha256;
use hpke::{Deserializable, Kem as KemTrait, OpModeR, OpModeS, Serializable};
use rand::RngCore;
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::share::{KeyPair, new_key_pair};
use crate::suite::{decode, encode};
use crate::{Error, Result};

type Kem = X25519HkdfSha256;

const HANDOFF_INFO: &[u8] = b"notes e2ee v1 sign-in handoff";

/// The longest export key sealed: OPAQUE's is 64 bytes.
const MAX_KEY: usize = 128;

fn info(challenge: &str) -> Vec<u8> {
    [HANDOFF_INFO, b"\0", challenge.as_bytes()].concat()
}

/// What an app keeps for one sign-in through the server's page, in memory
/// only: the PKCE verifier and its S256 challenge (RFC 7636), the state the
/// answer must bring back, and the key pair the export key is sealed to.
pub struct Handoff {
    /// The secret only the app knows, traded with the code.
    pub verifier: Zeroizing<String>,
    /// Its S256 hash, which the page and the code carry.
    pub challenge: String,
    /// Comes back with the page's answer, so a stray one is told apart.
    pub state: String,
    pair: KeyPair,
}

impl Handoff {
    /// New secrets for a sign-in.
    #[must_use]
    pub fn new() -> Self {
        let mut verifier = Zeroizing::new([0u8; 32]);
        OsRng.fill_bytes(verifier.as_mut());
        let verifier = Zeroizing::new(encode(verifier.as_slice()));
        let challenge = encode(&Sha256::digest(verifier.as_bytes()));
        let mut state = [0u8; 16];
        OsRng.fill_bytes(&mut state);
        Self {
            verifier,
            challenge,
            state: encode(&state),
            pair: new_key_pair(),
        }
    }

    /// The public key the page seals to.
    #[must_use]
    pub fn public_key(&self) -> String {
        self.pair.public_text()
    }

    /// The export key the page sealed for this sign-in.
    ///
    /// # Errors
    ///
    /// As [`open_export_key`].
    pub fn open(&self, sealed: &str) -> Result<Zeroizing<Vec<u8>>> {
        open_export_key(sealed, &self.pair, &self.challenge)
    }
}

impl Default for Handoff {
    fn default() -> Self {
        Self::new()
    }
}

/// The export key sealed for the app whose public key is `device_public`,
/// for the sign-in with this `challenge`.
///
/// # Errors
///
/// [`Error::Malformed`]: not a public key, or not an export key.
pub fn seal_export_key(export_key: &[u8], device_public: &str, challenge: &str) -> Result<String> {
    if export_key.is_empty() || export_key.len() > MAX_KEY {
        return Err(Error::Malformed("export key"));
    }
    let public = decode(device_public, "public key")?;
    let public = <Kem as KemTrait>::PublicKey::from_bytes(&public)
        .map_err(|_| Error::Malformed("public key"))?;
    let (encapped, sealed) = hpke::single_shot_seal::<ChaCha20Poly1305, HkdfSha256, Kem>(
        &OpModeS::Base,
        &public,
        &info(challenge),
        export_key,
        b"",
    )
    .map_err(|_| Error::Malformed("public key"))?;
    Ok(encode(&[encapped.to_bytes().as_slice(), &sealed].concat()))
}

/// The export key a sign-in page sealed for this key pair and challenge.
///
/// # Errors
///
/// [`Error::Malformed`] or [`Error::WrongKey`]: sealed for another key,
/// another sign-in, or changed on the way.
pub fn open_export_key(
    sealed: &str,
    pair: &KeyPair,
    challenge: &str,
) -> Result<Zeroizing<Vec<u8>>> {
    let bytes = decode(sealed, "sealed key")?;
    if bytes.len() < 32 {
        return Err(Error::Malformed("sealed key"));
    }
    let (encapped, sealed) = bytes.split_at(32);
    let encapped = <Kem as KemTrait>::EncappedKey::from_bytes(encapped)
        .map_err(|_| Error::Malformed("sealed key"))?;
    let private = <Kem as KemTrait>::PrivateKey::from_bytes(pair.private())
        .map_err(|_| Error::Malformed("private key"))?;
    let key = hpke::single_shot_open::<ChaCha20Poly1305, HkdfSha256, Kem>(
        &OpModeR::Base,
        &private,
        &encapped,
        &info(challenge),
        sealed,
        b"",
    )
    .map_err(|_| Error::WrongKey)?;
    Ok(Zeroizing::new(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_app_and_its_sign_in_open_the_key() {
        let app = new_key_pair();
        let export = [7u8; 64];
        let sealed = seal_export_key(&export, &app.public_text(), "challenge-a").unwrap();
        assert_eq!(
            open_export_key(&sealed, &app, "challenge-a")
                .unwrap()
                .as_slice(),
            export
        );
        assert!(
            open_export_key(&sealed, &app, "challenge-b").is_err(),
            "another sign-in"
        );
        assert!(
            open_export_key(&sealed, &new_key_pair(), "challenge-a").is_err(),
            "another app"
        );
        let mut changed = decode(&sealed, "sealed").unwrap();
        *changed.last_mut().unwrap() ^= 1;
        assert!(
            open_export_key(&encode(&changed), &app, "challenge-a").is_err(),
            "changed"
        );
        assert!(seal_export_key(&export, "not a key", "c").is_err());
        assert!(seal_export_key(&[], &app.public_text(), "c").is_err());
    }

    #[test]
    fn a_handoff_is_pkce_s256_and_opens_its_own_key() {
        let handoff = Handoff::new();
        assert_eq!(handoff.verifier.len(), 43);
        assert_eq!(
            handoff.challenge,
            encode(&Sha256::digest(handoff.verifier.as_bytes()))
        );
        assert_ne!(handoff.state, Handoff::new().state);
        let sealed =
            seal_export_key(&[3u8; 64], &handoff.public_key(), &handoff.challenge).unwrap();
        assert_eq!(handoff.open(&sealed).unwrap().as_slice(), [3u8; 64]);
    }
}
