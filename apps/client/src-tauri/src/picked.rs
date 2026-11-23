// SPDX-License-Identifier: AGPL-3.0-or-later

//! What the pickers give: a path on desktops, a `content://` address on
//! Android, which only the system can open (through the fs plugin) and
//! which has no folder beside it to write a temporary file in.

use std::fs::File;
use std::path::{Path, PathBuf};

use tauri::scope::fs::{Pattern, Scope};
use tauri::{AppHandle, Runtime};
use tauri_plugin_fs::{FilePath, FsExt, OpenOptions};

use crate::error::{CommandError, CommandResult};

/// An Android document, not a path.
pub fn is_document(path: &str) -> bool {
    path.starts_with("content://")
}

/// A path from the page, taken only when the person picked it in a dialog
/// during this run (the dialog adds each pick to the fs scope) and never a
/// network share or a `\\?\` path. A script in the page then reaches no
/// further than what the person chose.
pub fn picked_path<R: Runtime>(app: &AppHandle<R>, path: &str) -> CommandResult<PathBuf> {
    let scope = app.try_fs_scope();
    checked(path, |p| {
        scope
            .as_ref()
            .is_some_and(|s| s.is_allowed(p) || picked_as_given(s, p))
            || test_files(p)
    })
    .ok_or_else(|| CommandError::new("invalid", "not a picked file or folder"))
}

/// The scope finds the real path of an existing file first, which fails on
/// some network and shared drives (Windows error 1005 on a virtual
/// machine's host share), and then allows nothing. Such a path counts as
/// picked when a dialog allowed exactly it, as it was given: the file or
/// folder itself, never what a folder holds.
fn picked_as_given(scope: &Scope, path: &Path) -> bool {
    if std::fs::canonicalize(path).is_ok() {
        return false;
    }
    let given: PathBuf = path.components().collect();
    let forbidden = scope
        .forbidden_patterns()
        .iter()
        .any(|p| p.matches_path(&given));
    !forbidden && allowed_as_given(&scope.allowed_patterns(), &given)
}

/// Whether one of `allowed` is exactly `path`, with no wildcard.
fn allowed_as_given<'a>(allowed: impl IntoIterator<Item = &'a Pattern>, path: &Path) -> bool {
    let exact = Pattern::escape(&path.to_string_lossy());
    allowed.into_iter().any(|p| p.as_str() == exact)
}

/// End to end tests answer the pickers through the page (lib/pick.ts), so no
/// dialog adds their files to the scope: in debug builds only, a folder the
/// test names in `APP_TEST_FILES_DIR` counts as picked. Release builds have
/// no such door.
fn test_files(path: &Path) -> bool {
    #[cfg(debug_assertions)]
    if let Some(dir) = std::env::var_os("APP_TEST_FILES_DIR") {
        return !dir.is_empty() && path.starts_with(dir);
    }
    let _ = path;
    false
}

/// [`picked_path`]'s rules, `picked` saying whether the dialogs allowed it.
fn checked(path: &str, picked: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    if path.starts_with("\\\\") || path.starts_with("//") {
        return None;
    }
    let path = PathBuf::from(path);
    (path.is_absolute() && picked(&path)).then_some(path)
}

/// Opens a picked document, to read, or to write over.
pub fn open_document<R: Runtime>(
    app: &AppHandle<R>,
    uri: &str,
    write: bool,
) -> CommandResult<File> {
    let url =
        tauri::Url::parse(uri).map_err(|_| CommandError::new("invalid", "bad document address"))?;
    let mut options = OpenOptions::new();
    options.read(!write);
    if write {
        options.write(true).create(true).truncate(true);
    }
    app.fs()
        .open(FilePath::Url(url), options)
        .map_err(|e| CommandError::new("file", e.to_string()))
}

/// The name an address gives, when it gives one: the end of its last part,
/// decoded (`primary:Download/notes.enex` is `notes.enex`).
pub fn document_name(uri: &str) -> String {
    let last = uri.rsplit('/').next().unwrap_or(uri);
    let decoded = percent_decode(last);
    decoded
        .rsplit(['/', ':'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("export")
        .to_owned()
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = (bytes[i] == b'%')
            .then(|| text.get(i + 1..i + 3))
            .flatten()
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        if let Some(byte) = hex {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_picked_local_paths_pass() {
        let picked = |p: &Path| p.ends_with("theme.json");
        let local = if cfg!(windows) {
            "C:\\Users\\a\\theme.json"
        } else {
            "/home/a/theme.json"
        };
        assert!(checked(local, picked).is_some());
        assert!(checked(local, |_| false).is_none());
        assert!(checked("theme.json", picked).is_none());
        assert!(checked("\\\\server\\share\\theme.json", picked).is_none());
        assert!(checked("\\\\?\\C:\\theme.json", picked).is_none());
        assert!(checked("//server/share/theme.json", picked).is_none());
    }

    #[test]
    fn a_path_as_given_needs_its_own_pick() {
        let dir = if cfg!(windows) {
            "Z:\\share"
        } else {
            "/mnt/share"
        };
        let file = Path::new(dir).join("backup [1].ficelle");
        let allowed = [
            Pattern::new(&Pattern::escape(dir)).unwrap(),
            Pattern::new(&format!("{}/**", Pattern::escape(dir))).unwrap(),
        ];
        assert!(allowed_as_given(&allowed, Path::new(dir)));
        assert!(!allowed_as_given(&allowed, &file));
        let picked = [Pattern::new(&Pattern::escape(&file.to_string_lossy())).unwrap()];
        assert!(allowed_as_given(&picked, &file));
        assert!(!allowed_as_given(&picked, &file.with_extension("zip")));
    }

    #[test]
    fn names_come_from_the_address() {
        assert_eq!(
            document_name(
                "content://com.android.externalstorage.documents/document/primary%3ADownload%2Fnotes.enex"
            ),
            "notes.enex"
        );
        assert_eq!(
            document_name(
                "content://com.android.providers.downloads.documents/document/msf%3A1234"
            ),
            "1234"
        );
        assert!(is_document("content://x/y"));
        assert!(!is_document("C:/Users/a.zip"));
    }
}
