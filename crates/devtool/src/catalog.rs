// SPDX-License-Identifier: AGPL-3.0-or-later

//! The official catalog's tooling, for its owner (see `app_protocol::catalog`).
//!
//! `catalog keygen KEYFILE` makes the signing key: the private seed goes to
//! KEYFILE, kept off the repository and backed up, the public key is printed
//! for `brand.json` (`catalogKey`).
//!
//! `catalog sign FOLDER KEYFILE` reads `FOLDER/themes/*.json`, signs each
//! and writes `FOLDER/catalog.json`. Upload the whole folder to `catalogUrl`.

use std::fs;
use std::path::Path;

use app_core::catalog::{Catalog, CatalogEntry, CatalogKind, FORMAT};
use app_protocol::catalog::{hex_bytes, signed_message};
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, b| {
        use std::fmt::Write as _;
        // writing to a String cannot fail
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// Makes a key: the seed in `keyfile` (refused when it exists), the public
/// key returned.
pub fn keygen(keyfile: &Path) -> Result<String, String> {
    if keyfile.exists() {
        return Err(format!(
            "{} exists already, a new key would replace it",
            keyfile.display()
        ));
    }
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|e| e.to_string())?;
    let key = SigningKey::from_bytes(&seed);
    fs::write(keyfile, hex(&seed)).map_err(|e| e.to_string())?;
    Ok(hex(key.verifying_key().as_bytes()))
}

pub(crate) fn signing_key(keyfile: &Path) -> Result<SigningKey, String> {
    let text = fs::read_to_string(keyfile).map_err(|e| format!("{}: {e}", keyfile.display()))?;
    let seed = hex_bytes::<32>(text.trim()).ok_or("the key file does not hold a key")?;
    Ok(SigningKey::from_bytes(&seed))
}

fn sign(entry: &mut CatalogEntry, key: &SigningKey) {
    entry.signature = hex(&key.sign(&signed_message(entry)).to_bytes());
}

fn files(dir: &Path, extension: &str) -> Result<Vec<std::path::PathBuf>, String> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out: Vec<_> = fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == extension))
        .collect();
    out.sort();
    Ok(out)
}

/// Signs the themes in `folder` and writes its `catalog.json`. Returns the
/// number of themes.
pub fn sign_folder(folder: &Path, keyfile: &Path, now: i64) -> Result<usize, String> {
    let key = signing_key(keyfile)?;
    let mut entries = Vec::new();
    let mut themes = 0;
    for path in files(&folder.join("themes"), "json")? {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let bytes = fs::read(&path).map_err(|e| e.to_string())?;
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|e| format!("{name}: {e}"))?;
        let field = |f: &str| value.get(f).and_then(|v| v.as_str()).map(str::to_owned);
        let id = field("id").ok_or(format!("{name}: no id"))?;
        let sha = hex(&Sha256::digest(&bytes));
        let mut entry = CatalogEntry {
            kind: CatalogKind::Theme,
            id,
            name: field("name").ok_or(format!("{name}: no name"))?,
            version: sha[..12].to_owned(),
            description: field("description"),
            author: None,
            file: format!("themes/{name}"),
            size: bytes.len() as u64,
            file_sha256: sha.clone(),
            content_sha256: sha,
            signature: String::new(),
        };
        sign(&mut entry, &key);
        entries.push(entry);
        themes += 1;
    }
    entries.sort_by_key(|e| e.name.to_lowercase());
    let catalog = Catalog {
        format: FORMAT,
        generated_at: now,
        entries,
    };
    let text = serde_json::to_string_pretty(&catalog).map_err(|e| e.to_string())?;
    fs::write(folder.join("catalog.json"), text + "\n").map_err(|e| e.to_string())?;
    Ok(themes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signed_folder_checks_out_with_the_printed_key() {
        let dir = tempfile::tempdir().unwrap();
        let keyfile = dir.path().join("catalog.key");
        let public = keygen(&keyfile).unwrap();
        assert!(keygen(&keyfile).is_err(), "never replaces a key");

        let folder = dir.path().join("site");
        fs::create_dir_all(folder.join("themes")).unwrap();
        fs::write(
            folder.join("themes/ocean.json"),
            r#"{"id": "ocean", "name": "Ocean"}"#,
        )
        .unwrap();

        assert_eq!(sign_folder(&folder, &keyfile, 5).unwrap(), 1);
        let catalog: Catalog =
            serde_json::from_str(&fs::read_to_string(folder.join("catalog.json")).unwrap())
                .unwrap();
        let checked = app_core::catalog::checked(catalog, &public).unwrap();
        assert_eq!(checked.entries.len(), 1, "signed with the printed key");
        assert_eq!(checked.entries[0].kind, CatalogKind::Theme);
    }
}
