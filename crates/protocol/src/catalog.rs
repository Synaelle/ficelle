// SPDX-License-Identifier: AGPL-3.0-or-later

//! The official catalog: free themes the project's owner checked and
//! signed. A static folder hosted anywhere, `catalog.json`
//! beside the files it lists. Each entry is signed with the owner's Ed25519
//! key over [`signed_message`]: whoever serves the catalog (its host, an
//! instance that caches it) cannot change what gets installed without the
//! signature failing. The apps check it again before installing.

use serde::{Deserialize, Serialize};

/// The catalog format this build reads. 2: themes only, no plugin manifest
/// in the signed message.
pub const FORMAT: u32 = 2;

/// What an entry is: a theme, the only kind for now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CatalogKind {
    /// A theme file.
    Theme,
}

impl CatalogKind {
    /// As in the signed message and the routes.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Theme => "theme",
        }
    }
}

/// One theme of the catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    /// A theme.
    pub kind: CatalogKind,
    /// The theme's id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// The start of the theme file's hash.
    pub version: String,
    /// One line about it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Who made it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// The file, relative to the catalog's address.
    pub file: String,
    /// Its size in bytes.
    pub size: u64,
    /// Hex SHA-256 of the file as downloaded.
    pub file_sha256: String,
    /// Hex SHA-256 of what is installed, the theme file's own hash.
    pub content_sha256: String,
    /// Hex Ed25519 signature of [`signed_message`] by the owner's key.
    pub signature: String,
}

/// `catalog.json`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    /// [`FORMAT`].
    pub format: u32,
    /// When it was signed, unix milliseconds.
    pub generated_at: i64,
    /// Themes, by name.
    pub entries: Vec<CatalogEntry>,
}

/// What an instance says of the catalog to its apps: off, or what it holds
/// (entries already checked by the instance, checked again by the apps).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogView {
    /// The admin turned the catalog on.
    pub enabled: bool,
    /// The catalog as last fetched, `None` before the first fetch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog: Option<Catalog>,
    /// Why the last fetch failed, `None` when it went well.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// When it was last fetched, unix milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fetched_at: Option<i64>,
}

/// `PUT /api/v1/admin/catalog`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetCatalog {
    /// On or off for the whole instance.
    pub enabled: bool,
}

/// Compact JSON of a field, for [`signed_message`].
fn json<T: Serialize + ?Sized>(value: &T) -> String {
    // strings and options of strings always serialize
    serde_json::to_string(value).unwrap_or_default()
}

/// The bytes an entry's signature covers: every field but the signature,
/// so whoever serves the catalog can neither swap what is installed nor
/// relabel it (a name, an author, a description people read). Texts are
/// JSON quoted, one per line: a line break inside one cannot pass for the
/// next field. A signature for one entry is worthless for any other.
pub fn signed_message(entry: &CatalogEntry) -> Vec<u8> {
    [
        format!("catalog-{FORMAT}"),
        entry.kind.as_str().to_owned(),
        json(&entry.id),
        json(&entry.version),
        json(&entry.name),
        json(&entry.description),
        json(&entry.author),
        json(&entry.file),
        entry.size.to_string(),
        entry.file_sha256.clone(),
        entry.content_sha256.clone(),
    ]
    .join("\n")
    .into_bytes()
}

/// Whether a file path from a catalog stays inside its folder.
pub fn safe_file(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 200
        && !path.starts_with('/')
        && !path.contains("..")
        && !path.contains('\\')
        && !path.contains("://")
        && path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.'))
}

/// Hex text to bytes, `None` when it is not hex of that length.
pub fn hex_bytes<const N: usize>(text: &str) -> Option<[u8; N]> {
    if text.len() != N * 2 {
        return None;
    }
    let mut out = [0; N];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_paths_stay_in_the_catalog() {
        for ok in ["themes/ocean.json", "themes/dark-sea_2.json"] {
            assert!(safe_file(ok), "{ok}");
        }
        for bad in [
            "",
            "/etc/passwd",
            "../outside.json",
            "themes/../../x",
            "https://elsewhere.example/x.json",
            "themes\\x.json",
            "themes/a b.json",
        ] {
            assert!(!safe_file(bad), "{bad}");
        }
    }

    #[test]
    fn hex_reads_only_its_length() {
        assert_eq!(hex_bytes::<2>("0aff"), Some([10, 255]));
        assert_eq!(hex_bytes::<2>("0af"), None);
        assert_eq!(hex_bytes::<2>("0agf"), None);
    }
}
