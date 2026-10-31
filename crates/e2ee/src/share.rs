// SPDX-License-Identifier: AGPL-3.0-or-later

//! Sharing between e2ee accounts.
//!
//! - Each account has an X25519 key pair. The private key is sealed with a
//!   key derived from the account master key, so every device of the
//!   account opens it and the server cannot; the public key is published.
//! - A shared space has a random key per epoch. Its documents' keys derive
//!   from it as the personal space's derive from the master key
//!   ([`space_doc_key`]).
//! - The space key is sealed for each member with HPKE (RFC 9180): DHKEM
//!   X25519 HKDF-SHA256, HKDF-SHA256, ChaCha20-Poly1305, auth mode with the
//!   owner's key pair, so only the owner can make a grant a member takes. The
//!   info binds a grant to its space and epoch, so a grant cannot be passed
//!   off as another space's. Grants made before 0.3.0 used base mode, which
//!   anyone holding the member's public key could make, the server too;
//!   [`accept_unsigned`] still opens them, for a device that has no key of
//!   the space yet.
//! - [`fingerprint`] turns a public key into digits two people can compare,
//!   in case a server handed out a key that is not the person's.

use hpke::aead::ChaCha20Poly1305;
use hpke::kdf::HkdfSha256;
use hpke::kem::X25519HkdfSha256;
use hpke::{Deserializable, Kem as KemTrait, OpModeR, OpModeS, Serializable};
use rand::RngCore;
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::keys::MasterKey;
use crate::suite::{decode, encode};
use crate::sync::{DocKey, derive_with, open_sealed, seal_with};
use crate::{Error, Result};

type Kem = X25519HkdfSha256;

const PRIVATE_KEY: &[u8] = b"notes e2ee v1 private key";
const GRANT_INFO: &[u8] = b"notes e2ee v1 space key";
const OWNER_GRANT_INFO: &[u8] = b"notes e2ee v2 owner space key";

/// An account's key pair.
pub struct KeyPair {
    /// Published.
    pub public: [u8; 32],
    private: Zeroizing<[u8; 32]>,
}

impl std::fmt::Debug for KeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyPair")
            .field("public", &encode(&self.public))
            .finish_non_exhaustive()
    }
}

impl KeyPair {
    /// The private half, for the client's own encrypted storage only.
    pub fn private(&self) -> &[u8; 32] {
        &self.private
    }

    /// A key pair from its two halves as a client stored them.
    ///
    /// # Errors
    ///
    /// [`Error::Malformed`] or [`Error::WrongKey`]: halves that do not
    /// belong together.
    pub fn from_parts(public: &str, private: &[u8; 32]) -> Result<Self> {
        let public: [u8; 32] = decode(public, "public key")?
            .try_into()
            .map_err(|_| Error::Malformed("public key"))?;
        let derived = <Kem as KemTrait>::PrivateKey::from_bytes(private)
            .map_err(|_| Error::Malformed("private key"))?;
        if <Kem as KemTrait>::sk_to_pk(&derived).to_bytes().as_slice() != public.as_slice() {
            return Err(Error::WrongKey);
        }
        Ok(Self {
            public,
            private: Zeroizing::new(*private),
        })
    }

    /// The public half as published.
    pub fn public_text(&self) -> String {
        encode(&self.public)
    }
}

/// Whether this is a public key, for a server that cannot do more than
/// check its shape.
pub fn public_key_ok(public: &str) -> bool {
    decode(public, "public key").is_ok_and(|b| b.len() == 32)
}

/// A new key pair for an account.
pub fn new_key_pair() -> KeyPair {
    let (private, public) = Kem::gen_keypair();
    let mut secret = Zeroizing::new([0u8; 32]);
    secret.copy_from_slice(&private.to_bytes());
    let mut published = [0u8; 32];
    published.copy_from_slice(&public.to_bytes());
    KeyPair {
        public: published,
        private: secret,
    }
}

/// The private key as kept on the server, sealed for the account.
pub fn seal_key_pair(master: &MasterKey, pair: &KeyPair) -> String {
    let key = derive_with(master.as_bytes(), PRIVATE_KEY);
    encode(&seal_with(&key, pair.private.as_slice(), &pair.public))
}

/// The key pair from its sealed private key and its public key.
///
/// # Errors
///
/// [`Error::Malformed`] or [`Error::WrongKey`]: another account's, or a
/// public key that is not this pair's.
pub fn open_key_pair(master: &MasterKey, public: &str, sealed: &str) -> Result<KeyPair> {
    let public: [u8; 32] = decode(public, "public key")?
        .try_into()
        .map_err(|_| Error::Malformed("public key"))?;
    let key = derive_with(master.as_bytes(), PRIVATE_KEY);
    let private = open_sealed(&key, &decode(sealed, "private key")?, &public)?;
    let private: [u8; 32] = private
        .as_slice()
        .try_into()
        .map_err(|_| Error::Malformed("private key"))?;
    // the pair must hold together: a public key swapped next to a sealed
    // private key would leave grants unreadable
    let derived = <Kem as KemTrait>::PrivateKey::from_bytes(&private)
        .map_err(|_| Error::Malformed("private key"))?;
    if <Kem as KemTrait>::sk_to_pk(&derived).to_bytes().as_slice() != public.as_slice() {
        return Err(Error::WrongKey);
    }
    Ok(KeyPair {
        public,
        private: Zeroizing::new(private),
    })
}

/// A shared space's key for one epoch.
pub type SpaceKey = Zeroizing<[u8; 32]>;

/// A new random space key.
pub fn new_space_key() -> SpaceKey {
    let mut key = Zeroizing::new([0u8; 32]);
    OsRng.fill_bytes(key.as_mut());
    key
}

/// A document's key in a shared space.
pub fn space_doc_key(space_key: &SpaceKey, doc: &str) -> DocKey {
    crate::sync::doc_key_from(space_key.as_slice(), doc)
}

fn grant_info(label: &[u8], space: &str, epoch: u32) -> Vec<u8> {
    [label, b"\0", space.as_bytes(), b"\0", &epoch.to_be_bytes()].concat()
}

/// The space key sealed for a member, by their public key, and made by the
/// owner: only the holder of `owner`'s private key makes a grant that
/// [`accept`] takes as theirs.
///
/// # Errors
///
/// [`Error::Malformed`]: not a public key.
pub fn grant(
    space: &str,
    epoch: u32,
    key: &SpaceKey,
    recipient: &str,
    owner: &KeyPair,
) -> Result<String> {
    let public = public_of(recipient)?;
    let sender = (
        <Kem as KemTrait>::PrivateKey::from_bytes(owner.private.as_slice())
            .map_err(|_| Error::Malformed("private key"))?,
        <Kem as KemTrait>::PublicKey::from_bytes(&owner.public)
            .map_err(|_| Error::Malformed("public key"))?,
    );
    let (encapped, sealed) = hpke::single_shot_seal::<ChaCha20Poly1305, HkdfSha256, Kem>(
        &OpModeS::Auth(sender),
        &public,
        &grant_info(OWNER_GRANT_INFO, space, epoch),
        key.as_slice(),
        b"",
    )
    .map_err(|_| Error::Malformed("public key"))?;
    Ok(encode(&[encapped.to_bytes().as_slice(), &sealed].concat()))
}

fn public_of(text: &str) -> Result<<Kem as KemTrait>::PublicKey> {
    let public = decode(text, "public key")?;
    <Kem as KemTrait>::PublicKey::from_bytes(&public).map_err(|_| Error::Malformed("public key"))
}

/// The space key from a grant made for this account by the owner whose
/// public key is `owner`.
///
/// # Errors
///
/// [`Error::Malformed`] or [`Error::WrongKey`]: made for someone else, by
/// someone else than the owner, or for another space or epoch.
pub fn accept(
    space: &str,
    epoch: u32,
    grant: &str,
    pair: &KeyPair,
    owner: &str,
) -> Result<SpaceKey> {
    let owner = public_of(owner)?;
    open_grant(
        &OpModeR::Auth(owner),
        &grant_info(OWNER_GRANT_INFO, space, epoch),
        grant,
        pair,
    )
}

/// The space key from a grant made before 0.3.0, which anyone could have
/// made: only for a device that holds no key of the space yet and so trusts
/// the server's list as it is.
///
/// # Errors
///
/// As [`accept`].
pub fn accept_unsigned(space: &str, epoch: u32, grant: &str, pair: &KeyPair) -> Result<SpaceKey> {
    open_grant(
        &OpModeR::Base,
        &grant_info(GRANT_INFO, space, epoch),
        grant,
        pair,
    )
}

fn open_grant(mode: &OpModeR<Kem>, info: &[u8], grant: &str, pair: &KeyPair) -> Result<SpaceKey> {
    let bytes = decode(grant, "grant")?;
    if bytes.len() < 32 {
        return Err(Error::Malformed("grant"));
    }
    let (encapped, sealed) = bytes.split_at(32);
    let encapped = <Kem as KemTrait>::EncappedKey::from_bytes(encapped)
        .map_err(|_| Error::Malformed("grant"))?;
    let private = <Kem as KemTrait>::PrivateKey::from_bytes(pair.private.as_slice())
        .map_err(|_| Error::Malformed("private key"))?;
    let key = hpke::single_shot_open::<ChaCha20Poly1305, HkdfSha256, Kem>(
        mode, &private, &encapped, info, sealed, b"",
    )
    .map_err(|_| Error::WrongKey)?;
    let key: [u8; 32] = key
        .as_slice()
        .try_into()
        .map_err(|_| Error::Malformed("grant"))?;
    Ok(Zeroizing::new(key))
}

/// A public key as 20 digits in groups of five, the same for everyone who
/// looks at the same key.
///
/// # Errors
///
/// [`Error::Malformed`]: not a public key.
pub fn fingerprint(public: &str) -> Result<String> {
    let bytes = decode(public, "public key")?;
    let hash = Sha256::digest([b"notes e2ee v1 fingerprint\0".as_slice(), &bytes].concat());
    let groups: Vec<String> = hash
        .chunks(4)
        .take(4)
        .map(|c| {
            format!(
                "{:05}",
                u32::from_be_bytes([c[0], c[1], c[2], c[3]]) % 100_000
            )
        })
        .collect();
    Ok(groups.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grant_opens_for_its_member_space_and_epoch_only() {
        let master = MasterKey::generate();
        let alice = new_key_pair();
        let bob = new_key_pair();
        let sealed = seal_key_pair(&master, &alice);
        let public = encode(&alice.public);
        let opened = open_key_pair(&master, &public, &sealed).unwrap();
        assert_eq!(opened.public, alice.public);
        assert!(open_key_pair(&MasterKey::generate(), &public, &sealed).is_err());
        assert!(
            open_key_pair(&master, &encode(&bob.public), &sealed).is_err(),
            "a public key swapped next to the sealed private key"
        );

        let key = new_space_key();
        let alice_pub = encode(&alice.public);
        let to_bob = grant("s1", 1, &key, &encode(&bob.public), &alice).unwrap();
        assert_eq!(*accept("s1", 1, &to_bob, &bob, &alice_pub).unwrap(), *key);
        assert!(
            accept("s1", 1, &to_bob, &alice, &alice_pub).is_err(),
            "someone else's"
        );
        assert!(
            accept("s2", 1, &to_bob, &bob, &alice_pub).is_err(),
            "another space"
        );
        assert!(
            accept("s1", 2, &to_bob, &bob, &alice_pub).is_err(),
            "another epoch"
        );
        assert_ne!(
            to_bob,
            grant("s1", 1, &key, &encode(&bob.public), &alice).unwrap(),
            "fresh each time"
        );
        assert!(grant("s1", 1, &key, "not a key", &alice).is_err());

        let doc = space_doc_key(&key, "n1");
        assert_ne!(*doc, *space_doc_key(&key, "n2"));
        assert_ne!(*doc, *space_doc_key(&new_space_key(), "n1"));
    }

    #[test]
    fn only_the_owner_makes_a_grant_members_take() {
        let alice = new_key_pair();
        let bob = new_key_pair();
        let server = new_key_pair();
        let planted = new_space_key();
        // the server knows bob's public key and makes its own grant
        let forged = grant("s1", 9, &planted, &encode(&bob.public), &server).unwrap();
        assert!(accept("s1", 9, &forged, &bob, &encode(&alice.public)).is_err());
        assert!(
            accept_unsigned("s1", 9, &forged, &bob).is_err(),
            "an owner grant is not an old one"
        );
    }

    #[test]
    fn grants_from_before_owners_signed_still_open_unsigned() {
        let bob = new_key_pair();
        let key = new_space_key();
        let public = public_of(&encode(&bob.public)).unwrap();
        let (encapped, sealed) = hpke::single_shot_seal::<ChaCha20Poly1305, HkdfSha256, Kem>(
            &OpModeS::Base,
            &public,
            &grant_info(GRANT_INFO, "s1", 1),
            key.as_slice(),
            b"",
        )
        .unwrap();
        let old = encode(&[encapped.to_bytes().as_slice(), &sealed].concat());
        assert_eq!(*accept_unsigned("s1", 1, &old, &bob).unwrap(), *key);
        let anyone = encode(&new_key_pair().public);
        assert!(accept("s1", 1, &old, &bob, &anyone).is_err());
    }

    #[test]
    fn fingerprints_are_stable_and_differ_by_key() {
        let a = encode(&new_key_pair().public);
        let b = encode(&new_key_pair().public);
        let fa = fingerprint(&a).unwrap();
        assert_eq!(fa, fingerprint(&a).unwrap());
        assert_ne!(fa, fingerprint(&b).unwrap());
        assert_eq!(fa.len(), 23);
        assert!(
            fa.split(' ')
                .all(|g| g.len() == 5 && g.chars().all(|c| c.is_ascii_digit()))
        );
    }
}
