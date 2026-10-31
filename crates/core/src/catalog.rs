// SPDX-License-Identifier: AGPL-3.0-or-later

//! Checking the official catalog (see `app_protocol::catalog`) on this
//! device, whoever served it: each entry's signature against the key built
//! into the app, then a downloaded file against the hashes the signature
//! covers.

pub use app_protocol::catalog::{Catalog, CatalogEntry, CatalogKind, FORMAT};
use app_protocol::catalog::{hex_bytes, safe_file, signed_message};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};

use crate::{Error, Result};

fn key(public_key: &str) -> Option<VerifyingKey> {
    VerifyingKey::from_bytes(&hex_bytes::<32>(public_key)?).ok()
}

/// Whether an entry was signed by the owner of `public_key` and names a
/// file inside the catalog.
pub fn entry_signed(entry: &CatalogEntry, public_key: &str) -> bool {
    let (Some(key), Some(signature)) = (key(public_key), hex_bytes::<64>(&entry.signature)) else {
        return false;
    };
    safe_file(&entry.file)
        && key
            .verify(&signed_message(entry), &Signature::from_bytes(&signature))
            .is_ok()
}

/// The catalog's entries that check out, the others dropped.
///
/// # Errors
///
/// `Invalid` for a catalog of another format or a key that is not one.
pub fn checked(mut catalog: Catalog, public_key: &str) -> Result<Catalog> {
    if key(public_key).is_none() {
        return Err(Error::Invalid("this build has no catalog key".into()));
    }
    if catalog.format != FORMAT {
        return Err(Error::Invalid(format!(
            "catalog format {} is not read here",
            catalog.format
        )));
    }
    catalog.entries.retain(|e| entry_signed(e, public_key));
    Ok(catalog)
}

fn hex_sha256(bytes: &[u8]) -> String {
    crate::blobs::hex(&Sha256::digest(bytes))
}

/// A downloaded file is the one the signed entry names.
///
/// # Errors
///
/// `Invalid` when the entry is not signed or the file is not its file.
pub fn check_file(entry: &CatalogEntry, bytes: &[u8], public_key: &str) -> Result<()> {
    if !entry_signed(entry, public_key) {
        return Err(Error::Invalid("the catalog entry is not signed".into()));
    }
    if hex_sha256(bytes) != entry.file_sha256 {
        return Err(Error::Invalid(
            "the file is not the one the catalog signed".into(),
        ));
    }
    if entry.kind == CatalogKind::Theme && entry.content_sha256 != entry.file_sha256 {
        return Err(Error::Invalid("a theme is installed as downloaded".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};

    use super::*;

    const SEED: [u8; 32] = [7; 32];

    fn public() -> String {
        crate::blobs::hex(SigningKey::from_bytes(&SEED).verifying_key().as_bytes())
    }

    fn sign(entry: &mut CatalogEntry, seed: [u8; 32]) {
        let signature = SigningKey::from_bytes(&seed).sign(&signed_message(entry));
        entry.signature = crate::blobs::hex(&signature.to_bytes());
    }

    fn theme_entry(bytes: &[u8]) -> CatalogEntry {
        let sha = hex_sha256(bytes);
        let mut entry = CatalogEntry {
            kind: CatalogKind::Theme,
            id: "ocean".into(),
            name: "Ocean".into(),
            version: sha[..12].into(),
            description: None,
            author: None,
            file: "themes/ocean.json".into(),
            size: bytes.len() as u64,
            file_sha256: sha.clone(),
            content_sha256: sha,
            signature: String::new(),
        };
        sign(&mut entry, SEED);
        entry
    }

    #[test]
    fn only_entries_signed_by_the_key_are_kept() {
        let good = theme_entry(b"{}");
        let mut forged = theme_entry(b"{\"x\":1}");
        sign(&mut forged, [9; 32]);
        let mut changed = theme_entry(b"{}");
        changed.id = "other".into();
        let mut escaping = theme_entry(b"{}");
        escaping.file = "../../outside.json".into();
        sign(&mut escaping, SEED);
        let catalog = Catalog {
            format: FORMAT,
            generated_at: 0,
            entries: vec![good.clone(), forged, changed, escaping],
        };
        let kept = checked(catalog.clone(), &public()).unwrap();
        assert_eq!(kept.entries, vec![good]);
        assert!(
            checked(
                Catalog {
                    format: FORMAT + 1,
                    ..catalog.clone()
                },
                &public()
            )
            .is_err()
        );
        assert!(checked(catalog, "").is_err(), "no key, no catalog");
    }

    #[test]
    fn every_field_is_signed_so_nothing_can_be_relabelled() {
        let good = theme_entry(b"{}");
        let changes: [fn(&mut CatalogEntry); 7] = [
            |e| e.name = "Bank helper".into(),
            |e| e.description = Some("Official".into()),
            |e| e.author = Some("Someone else".into()),
            |e| e.version = "2.0.0".into(),
            |e| e.file = "themes/other.json".into(),
            |e| e.size += 1,
            // a line break cannot move text from one field into the next
            |e| {
                e.name = "Ocean\n\"x\"".into();
                e.description = None;
            },
        ];
        for (i, change) in changes.iter().enumerate() {
            let mut entry = good.clone();
            change(&mut entry);
            assert!(
                !entry_signed(&entry, &public()),
                "change {i} kept its signature"
            );
        }
        assert!(entry_signed(&good, &public()));
    }

    #[test]
    fn a_file_must_be_the_one_signed() {
        let entry = theme_entry(b"{}");
        check_file(&entry, b"{}", &public()).unwrap();
        assert!(check_file(&entry, b"{ }", &public()).is_err());
        let mut other = entry.clone();
        other.signature = "00".repeat(64);
        assert!(check_file(&other, b"{}", &public()).is_err());
    }
}
