// SPDX-License-Identifier: AGPL-3.0-or-later

//! What the pickers give: a path on desktops, a `content://` address on
//! Android, which only the system can open (through the fs plugin) and
//! which has no folder beside it to write a temporary file in.

use std::fs::File;
use std::path::{Path, PathBuf};

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
    checked(path, |p| scope.as_ref().is_some_and(|s| s.is_allowed(p)))
        .ok_or_else(|| CommandError::new("invalid", "not a picked file or folder"))
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
