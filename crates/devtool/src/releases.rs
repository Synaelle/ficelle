// SPDX-License-Identifier: AGPL-3.0-or-later

//! The signed release list servers check for update notices (see
//! `app_protocol::releases`).
//!
//! `releases sign FILE KEYFILE` checks every entry of FILE (a version
//! `major.minor.micro`, a date `YYYY-MM-DD`), signs each with the key
//! `catalog keygen` made (the same key may serve both, the signed messages
//! differ) and writes FILE back, newest first. Upload it to `releasesUrl`.

use std::fs;
use std::path::Path;

use app_protocol::releases::{FORMAT, ReleaseList, signed_message, version_parts};
use ed25519_dalek::Signer;

use crate::catalog::{hex, signing_key};

fn plausible_date(date: &str) -> bool {
    let bytes = date.as_bytes();
    date.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && date
            .chars()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

/// Signs every release in `file`, returns how many.
pub fn sign_file(file: &Path, keyfile: &Path) -> Result<usize, String> {
    let key = signing_key(keyfile)?;
    let text = fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let mut list: ReleaseList =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))?;
    if list.format == 0 {
        list.format = FORMAT;
    }
    if list.format != FORMAT {
        return Err(format!(
            "format {} is not this tool's {FORMAT}",
            list.format
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for release in &mut list.releases {
        if version_parts(&release.version).is_none() {
            return Err(format!("{:?} is not a version like 0.1.0", release.version));
        }
        if !seen.insert(release.version.clone()) {
            return Err(format!("{} is there twice", release.version));
        }
        if !plausible_date(&release.date) {
            return Err(format!(
                "{}: {:?} is not a date like 2026-11-01",
                release.version, release.date
            ));
        }
        release.signature = hex(&key.sign(&signed_message(release)).to_bytes());
    }
    list.releases
        .sort_by_key(|r| std::cmp::Reverse(version_parts(&r.version)));
    let out = serde_json::to_string_pretty(&list).map_err(|e| e.to_string())? + "\n";
    fs::write(file, out).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(list.releases.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signature, Verifier};

    #[test]
    fn every_entry_is_checked_signed_and_sorted() {
        let dir = tempfile::tempdir().unwrap();
        let keyfile = dir.path().join("key");
        let public = crate::catalog::keygen(&keyfile).unwrap();
        let file = dir.path().join("releases.json");
        fs::write(
            &file,
            r#"{ "releases": [
                { "version": "0.1.0", "date": "2026-11-01" },
                { "version": "0.2.0", "date": "2026-12-01", "security": true, "notes": "a fix" }
            ] }"#,
        )
        .unwrap();
        assert_eq!(sign_file(&file, &keyfile).unwrap(), 2);
        let list: ReleaseList = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(list.format, FORMAT);
        assert_eq!(list.releases[0].version, "0.2.0", "newest first");
        let key = ed25519_dalek::VerifyingKey::from_bytes(
            &app_protocol::catalog::hex_bytes::<32>(&public).unwrap(),
        )
        .unwrap();
        for release in &list.releases {
            let signature = app_protocol::catalog::hex_bytes::<64>(&release.signature).unwrap();
            assert!(
                key.verify(&signed_message(release), &Signature::from_bytes(&signature))
                    .is_ok()
            );
        }

        for (bad, why) in [
            (
                r#"{ "releases": [{ "version": "1.0", "date": "2026-11-01" }] }"#,
                "version",
            ),
            (
                r#"{ "releases": [{ "version": "1.0.0", "date": "11/01/2026" }] }"#,
                "date",
            ),
            (
                r#"{ "releases": [{ "version": "1.0.0", "date": "2026-11-01" }, { "version": "1.0.0", "date": "2026-11-02" }] }"#,
                "twice",
            ),
        ] {
            fs::write(&file, bad).unwrap();
            assert!(
                sign_file(&file, &keyfile).unwrap_err().contains(why),
                "{why}"
            );
        }
    }
}
