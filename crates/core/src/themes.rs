// SPDX-License-Identifier: AGPL-3.0-or-later

//! Themes the person imported, one json file each in a folder beside the
//! vault's database. They are not secret and the lock screen needs them
//! before any unlock, so they stay outside the encrypted store. The page
//! checks a theme's content (colors, readability, CSS), this module only
//! keeps files sound: a safe id for the file name, a size cap, json whose
//! `id` is the file's.

use std::fs;
use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// A theme with its fonts embedded stays well under this.
pub const MAX_THEME: usize = 2 * 1024 * 1024;

/// Ids name files: lowercase letters, digits and dashes, as the theme schema says.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 40
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The imported themes kept in `dir`.
pub struct Themes {
    dir: PathBuf,
}

impl Themes {
    /// The themes kept in `dir`, made when the first one is saved.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Every theme file, as written, in name order. Files that are not
    /// themes (wrong name, too big, unreadable) are skipped.
    ///
    /// # Errors
    ///
    /// When the folder exists but cannot be read.
    pub fn list(&self) -> Result<Vec<String>> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.extension().is_some_and(|x| x == "json")
                    && p.file_stem().and_then(|s| s.to_str()).is_some_and(valid_id)
            })
            .collect();
        paths.sort();
        Ok(paths
            .iter()
            .filter(|p| fs::metadata(p).is_ok_and(|m| m.len() <= MAX_THEME as u64))
            .filter_map(|p| fs::read_to_string(p).ok())
            .collect())
    }

    /// Keeps a theme under its id, replacing the one there. Written beside
    /// and moved in place, so a crash never leaves half a theme.
    ///
    /// # Errors
    ///
    /// `Invalid` for too large a file, not json, or no valid `id`, else
    /// when the file cannot be written.
    pub fn save(&self, json: &str) -> Result<String> {
        if json.len() > MAX_THEME {
            return Err(Error::Invalid("the theme file is too large".into()));
        }
        let value: serde_json::Value = serde_json::from_str(json)
            .map_err(|e| Error::Invalid(format!("the theme is not json: {e}")))?;
        let id = value
            .get("id")
            .and_then(|v| v.as_str())
            .filter(|id| valid_id(id))
            .ok_or_else(|| Error::Invalid("the theme has no valid id".into()))?
            .to_owned();
        fs::create_dir_all(&self.dir)?;
        let partial = self.dir.join(format!("{id}.json.partial"));
        fs::write(&partial, json)?;
        fs::rename(&partial, self.path(&id))?;
        Ok(id)
    }

    /// Forgets a theme, nothing when it was not there.
    ///
    /// # Errors
    ///
    /// `Invalid` for an id that is not one, else when the file stays.
    pub fn remove(&self, id: &str) -> Result<()> {
        if !valid_id(id) {
            return Err(Error::Invalid("not a theme id".into()));
        }
        match fs::remove_file(self.path(id)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }

    fn path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    /// The folder, for messages and tests.
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme(id: &str) -> String {
        format!(r#"{{"id": "{id}", "name": "N"}}"#)
    }

    #[test]
    fn themes_are_kept_listed_replaced_and_removed() {
        let dir = tempfile::tempdir().unwrap();
        let themes = Themes::new(dir.path().join("themes"));
        assert!(themes.list().unwrap().is_empty(), "no folder yet");
        assert_eq!(themes.save(&theme("b")).unwrap(), "b");
        themes.save(&theme("a")).unwrap();
        assert_eq!(themes.list().unwrap(), vec![theme("a"), theme("b")]);
        let changed = r#"{"id": "a", "name": "Changed"}"#;
        themes.save(changed).unwrap();
        assert_eq!(themes.list().unwrap()[0], changed);
        themes.remove("a").unwrap();
        themes.remove("a").unwrap();
        assert_eq!(themes.list().unwrap(), vec![theme("b")]);
        assert!(!themes.dir().join("a.json.partial").exists());
    }

    #[test]
    fn bad_themes_are_refused_and_stray_files_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let themes = Themes::new(dir.path());
        for bad in [
            "not json".to_owned(),
            r#"{"name": "no id"}"#.to_owned(),
            theme("../escape"),
            theme("Upper"),
            theme(""),
            theme(&"a".repeat(41)),
            format!(r#"{{"id": "big", "css": "{}"}}"#, "a".repeat(MAX_THEME)),
        ] {
            assert!(
                matches!(themes.save(&bad), Err(Error::Invalid(_))),
                "{}",
                &bad[..bad.len().min(40)]
            );
        }
        assert!(themes.remove("../x").is_err());
        fs::write(dir.path().join("notes.txt"), "x").unwrap();
        fs::write(dir.path().join("Bad Name.json"), theme("x")).unwrap();
        fs::write(dir.path().join("huge.json"), "a".repeat(MAX_THEME + 1)).unwrap();
        assert!(themes.list().unwrap().is_empty());
    }
}
