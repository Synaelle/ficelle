// SPDX-License-Identifier: AGPL-3.0-or-later

//! Backups in a folder reached through Android's document access. A new
//! install may not read, by path, the files an earlier install made in
//! Documents: after a reinstall or a phone reset its old backups are out of
//! reach until the person picks their folder once in the system's picker
//! (BackupFolder.kt). The folder's address is then the backups' folder,
//! and backups restore from it and go on in it.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use app_core::{Error, Result};
use serde::{Deserialize, Serialize};
use tauri::plugin::{PluginHandle, TauriPlugin};
use tauri::{AppHandle, Wry};
use tauri_plugin_fs::{FilePath, FsExt, OpenOptions};

static HANDLE: OnceLock<PluginHandle<Wry>> = OnceLock::new();
/// For the fs plugin, which opens the files by their address.
static APP: OnceLock<AppHandle<Wry>> = OnceLock::new();

/// The in-app plugin that reaches `BackupFolderPlugin`. No permission names
/// it, so the page cannot call it.
pub fn plugin() -> TauriPlugin<Wry> {
    tauri::plugin::Builder::new("backup-folder")
        .setup(|app, api| {
            let _ = APP.set(app.clone());
            let handle =
                api.register_android_plugin("com.synaelle.ficelle.dev", "BackupFolderPlugin")?;
            let _ = HANDLE.set(handle);
            Ok(())
        })
        .build()
}

fn call<T: serde::de::DeserializeOwned>(command: &str, args: impl Serialize) -> Result<T> {
    let handle = HANDLE
        .get()
        .ok_or_else(|| Error::Io(std::io::Error::other("the folder access is not ready yet")))?;
    handle.run_mobile_plugin(command, args).map_err(|e| {
        let text = e.to_string();
        let kind = if text.contains("denied") {
            std::io::ErrorKind::PermissionDenied
        } else {
            std::io::ErrorKind::Other
        };
        Error::Io(std::io::Error::new(kind, format!("backups folder: {text}")))
    })
}

#[derive(Deserialize)]
struct Picked {
    tree: String,
}

/// Asks the person for the phone's backups folder through the system's
/// picker. None when they backed out.
///
/// # Errors
///
/// The picker did not open.
pub fn pick() -> Result<Option<String>> {
    match call::<Picked>("pick", ()) {
        Ok(picked) => Ok(Some(picked.tree)),
        Err(Error::Io(e)) if e.to_string().contains("cancelled") => Ok(None),
        Err(e) => Err(e),
    }
}

#[derive(Serialize)]
struct DocArgs<'a> {
    tree: &'a str,
    path: &'a str,
    name: &'a str,
}

#[derive(Deserialize)]
struct Found {
    uri: Option<String>,
}

#[derive(Deserialize)]
struct Files {
    files: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    path: String,
    size: u64,
}

/// The backups in a picked folder, or in their own folder inside it, as
/// on a desktop.
pub struct DocumentTarget {
    tree: String,
    /// The backups' folder in the picked one, "" or ending in `/`.
    base: String,
    /// Addresses found, by path: a provider is asked once per file.
    found: Mutex<HashMap<String, String>>,
}

impl DocumentTarget {
    /// The backups in the folder picked at `tree`. One holding the snapshot
    /// backups of an earlier version (their `config`) is their folder.
    ///
    /// # Errors
    ///
    /// The folder could not be read.
    pub fn named(tree: &str) -> Result<Self> {
        let chosen = crate::picked::document_name(tree);
        let name = app_core::repo::folder_name(&chosen);
        let mut target = Self {
            tree: tree.to_owned(),
            base: String::new(),
            found: Mutex::new(HashMap::new()),
        };
        if chosen != name && target.find("config")?.is_none() {
            target.base = format!("{name}/");
        }
        Ok(target)
    }

    fn at(&self, name: &str) -> Result<String> {
        if name.is_empty() || name.starts_with('.') || name.contains(['/', '\\']) {
            return Err(Error::Invalid(format!("not a backup file: {name}")));
        }
        Ok(format!("{}{name}", self.base))
    }

    fn args<'a>(&'a self, path: &'a str, name: &'a str) -> DocArgs<'a> {
        DocArgs {
            tree: &self.tree,
            path,
            name,
        }
    }

    fn known(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.found
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The address of the file at `path` (from the picked folder).
    fn find(&self, path: &str) -> Result<Option<String>> {
        if let Some(uri) = self.known().get(path) {
            return Ok(Some(uri.clone()));
        }
        let found = call::<Found>("find", self.args(path, ""))?.uri;
        if let Some(uri) = &found {
            self.known().insert(path.to_owned(), uri.clone());
        }
        Ok(found)
    }

    /// The file to read at `path`, or the one a replacement left at
    /// `path.old` when it stopped halfway (see [`Self::write_with`]).
    fn readable(&self, path: &str) -> Result<std::fs::File> {
        let uri = match self.find(path)? {
            Some(uri) => uri,
            None => self
                .find(&format!("{path}.old"))?
                .ok_or(Error::NotFound("backup file"))?,
        };
        Self::open(&uri, false)
    }

    fn open(uri: &str, write: bool) -> Result<std::fs::File> {
        let url = tauri::Url::parse(uri).map_err(|e| Error::Invalid(e.to_string()))?;
        let mut options = OpenOptions::new();
        options.read(!write);
        if write {
            options.write(true).truncate(true);
        }
        let app = APP.get().ok_or_else(|| {
            Error::Io(std::io::Error::other("the folder access is not ready yet"))
        })?;
        Ok(app.fs().open(FilePath::Url(url), options)?)
    }

    /// Writes a new file at `path`, beside it first, then renamed, its
    /// content given by `fill` as it goes.
    fn write_with<T>(
        &self,
        path: &str,
        fill: impl FnOnce(&mut std::fs::File) -> Result<T>,
    ) -> Result<T> {
        let name = path.rsplit('/').next().unwrap_or(path);
        let partial = format!("{path}.partial");
        let made = call::<Found>("create", self.args(&partial, ""))?;
        let uri = made
            .uri
            .ok_or_else(|| Error::Io(std::io::Error::other("the folder made no file")))?;
        let given = {
            let mut out = Self::open(&uri, true)?;
            let written = fill(&mut out).and_then(|given| {
                out.sync_all()?;
                Ok(given)
            });
            match written {
                Ok(given) => given,
                Err(e) => {
                    let _ = self.delete_file(&partial);
                    return Err(e);
                }
            }
        };
        // the provider would not replace a file by renaming over it: the
        // old one steps aside, and is read in its place until the new one
        // has its name
        self.known().remove(path);
        let old = format!("{path}.old");
        let replacing = call::<Found>("find", self.args(path, ""))?.uri.is_some();
        if replacing {
            self.delete_file(&old)?;
            call::<Found>("rename", self.args(path, &format!("{name}.old")))?;
        }
        let renamed = call::<Found>("rename", self.args(&partial, name))?;
        if let Some(uri) = renamed.uri {
            self.known().insert(path.to_owned(), uri);
        }
        if replacing {
            self.delete_file(&old)?;
        }
        Ok(given)
    }

    /// Writes the file `name` in the backups' folder, its content given by
    /// `fill` as it goes.
    ///
    /// # Errors
    ///
    /// The folder refused, or `fill` failed (no file is left then).
    pub fn put_with<T>(
        &self,
        name: &str,
        fill: impl FnOnce(&mut std::fs::File) -> Result<T>,
    ) -> Result<T> {
        self.write_with(&self.at(name)?, fill)
    }

    /// The file `name` in the backups' folder, to read.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`] when it is not there.
    pub fn open_named(&self, name: &str) -> Result<std::fs::File> {
        self.readable(&self.at(name)?)
    }

    /// The files in the backups' folder and below, by their path from it,
    /// with their sizes.
    ///
    /// # Errors
    ///
    /// The folder could not be read.
    pub fn list(&self) -> Result<Vec<(String, u64)>> {
        let files = call::<Files>("list", self.args(&self.base, ""))?.files;
        let mut out: Vec<(String, u64)> = files
            .into_iter()
            .filter_map(|f| Some((f.path.strip_prefix(&self.base)?.to_owned(), f.size)))
            .collect();
        out.sort();
        Ok(out)
    }

    /// Deletes the file `name` in the backups' folder.
    ///
    /// # Errors
    ///
    /// The folder refused.
    pub fn delete(&self, name: &str) -> Result<()> {
        self.delete_file(&self.at(name)?)
    }

    fn delete_file(&self, path: &str) -> Result<()> {
        self.known().remove(path);
        call::<serde_json::Value>("delete", self.args(path, "")).map(drop)
    }
}
