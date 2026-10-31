// SPDX-License-Identifier: AGPL-3.0-or-later

//! The themes an admin offers, and the instance's rule for them. A theme
//! file is kept as uploaded, read here only enough to list it; each device
//! checks it again before using it.

use app_protocol::extensions::{ExtensionRules, Extensions, OfferedTheme};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use crate::db::DbError;

/// A theme file, at most (its fonts embedded).
pub const MAX_THEME: usize = 2 * 1024 * 1024;

/// The route segment of what is offered: themes, the only kind for now.
pub const THEMES: &str = "themes";

fn hex_sha256(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut out, b| {
            // writing to a String cannot fail
            let _ = write!(out, "{b:02x}");
            out
        })
}

fn valid_id(id: &str) -> bool {
    (1..=40).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The rule, the default until the admin sets it.
///
/// # Errors
///
/// Database errors.
pub fn rules(conn: &Connection) -> Result<ExtensionRules, DbError> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'extensions'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    // a value that no longer reads gives the default, as an unset one
    Ok(value
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default())
}

/// Keeps the rule.
///
/// # Errors
///
/// Database errors.
pub fn set_rules(conn: &Connection, rules: &ExtensionRules) -> Result<(), DbError> {
    let value = serde_json::to_string(rules).unwrap_or_default();
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('extensions', ?1)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        [value],
    )?;
    Ok(())
}

/// Everything an app or the admin page reads: the rule and the themes
/// offered, by name.
///
/// # Errors
///
/// Database errors.
pub fn all(conn: &Connection) -> Result<Extensions, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, length(content), sha256, preinstall, uploaded_at
         FROM extensions WHERE kind = 'theme' ORDER BY lower(name), id",
    )?;
    let themes = stmt
        .query_map([], |r| {
            Ok(OfferedTheme {
                id: r.get(0)?,
                name: r.get(1)?,
                size: u64::try_from(r.get::<_, i64>(2)?).unwrap_or(0),
                sha256: r.get(3)?,
                preinstall: r.get(4)?,
                uploaded_at: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Extensions {
        rules: rules(conn)?,
        themes,
    })
}

/// What an upload could not be.
#[derive(Debug, PartialEq, Eq)]
pub struct Refused(pub String);

/// Reads a theme file enough to list it: json with an id and a name. Its
/// colours and css are checked by the admin page before the upload and by
/// each app before it uses it.
///
/// # Errors
///
/// [`Refused`] naming what is wrong.
pub fn read_theme(bytes: &[u8]) -> Result<(String, String), Refused> {
    if bytes.len() > MAX_THEME {
        return Err(Refused("a theme is at most 2 MB".into()));
    }
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| Refused("the theme is not json".into()))?;
    let id = value
        .get("id")
        .and_then(|v| v.as_str())
        .filter(|id| valid_id(id))
        .ok_or_else(|| Refused("the theme has no valid id".into()))?;
    let name = value
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|n| !n.is_empty() && n.chars().count() <= 60)
        .ok_or_else(|| Refused("the theme has no name".into()))?;
    Ok((id.to_owned(), name.to_owned()))
}

/// A theme upload, read and checked.
pub struct Upload<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub content: &'a [u8],
    pub preinstall: bool,
}

/// Keeps a theme, replacing the one with its id. Returns its hash.
///
/// # Errors
///
/// Database errors.
pub fn put(conn: &Connection, upload: &Upload<'_>, now: i64) -> Result<String, DbError> {
    let Upload {
        id,
        name,
        content,
        preinstall,
    } = *upload;
    let sha256 = hex_sha256(content);
    conn.execute(
        "INSERT INTO extensions (kind, id, name, meta, content, sha256, preinstall, uploaded_at)
         VALUES ('theme', ?1, ?2, '{}', ?3, ?4, ?5, ?6)
         ON CONFLICT (kind, id) DO UPDATE SET name = ?2, content = ?3,
             sha256 = ?4, preinstall = ?5, uploaded_at = ?6",
        params![id, name, content, sha256, preinstall, now],
    )?;
    Ok(sha256)
}

/// Offered on every device or only listed. False when there is no such theme.
///
/// # Errors
///
/// Database errors.
pub fn set_preinstall(conn: &Connection, id: &str, preinstall: bool) -> Result<bool, DbError> {
    Ok(conn.execute(
        "UPDATE extensions SET preinstall = ?2 WHERE kind = 'theme' AND id = ?1",
        params![id, preinstall],
    )? > 0)
}

/// Takes a theme away. False when there was none.
///
/// # Errors
///
/// Database errors.
pub fn remove(conn: &Connection, id: &str) -> Result<bool, DbError> {
    Ok(conn.execute(
        "DELETE FROM extensions WHERE kind = 'theme' AND id = ?1",
        [id],
    )? > 0)
}

/// A theme file as uploaded.
///
/// # Errors
///
/// Database errors.
pub fn content(conn: &Connection, id: &str) -> Result<Option<Vec<u8>>, DbError> {
    Ok(conn
        .query_row(
            "SELECT content FROM extensions WHERE kind = 'theme' AND id = ?1",
            [id],
            |r| r.get(0),
        )
        .optional()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn themes_need_an_id_and_a_name() {
        assert_eq!(
            read_theme(br#"{"id": "ocean", "name": "Ocean"}"#).unwrap(),
            ("ocean".to_owned(), "Ocean".to_owned())
        );
        for bad in [
            &b"not json"[..],
            br#"{"name": "No id"}"#,
            br#"{"id": "../x", "name": "Bad"}"#,
            br#"{"id": "ok", "name": " "}"#,
        ] {
            assert!(read_theme(bad).is_err());
        }
    }
}
