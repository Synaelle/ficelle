// SPDX-License-Identifier: AGPL-3.0-or-later

//! Backups in a folder reached through Android's document access. A new
//! install may not read, by path, the files an earlier install made in
//! Documents: after a reinstall or a phone reset its old backups are out of
//! reach until the person picks their folder once in the system's picker
//! (BackupFolder.kt). The folder's address is then the backup target's path,
//! and backups restore from it and go on in it.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::{Mutex, OnceLock};

use app_repo::target::safe_path;
use app_repo::{Error, Listed, Result, Target};
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

/// A repository in a picked folder, or in its own folder inside it, as
/// [`app_repo::FolderTarget::named`] does.
pub struct DocumentTarget {
    tree: String,
    /// The repository's folder in the picked one, "" or ending in `/`.
    base: String,
    readme: String,
    /// Addresses found, by path: a provider is asked once per file.
    found: Mutex<HashMap<String, String>>,
}

impl DocumentTarget {
    /// The repository in the folder picked at `tree`.
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
            readme: app_core::repo::readme(),
            found: Mutex::new(HashMap::new()),
        };
        if chosen != name && target.find("config")?.is_none() {
            target.base = format!("{name}/");
        }
        Ok(target)
    }

    fn at(&self, path: &str) -> Result<String> {
        if !safe_path(path) {
            return Err(Error::Invalid(format!("not a repository path: {path}")));
        }
        Ok(format!("{}{path}", self.base))
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
    /// `path.old` when it stopped halfway (see [`Target::put`]).
    fn readable(&self, path: &str) -> Result<std::fs::File> {
        let uri = match self.find(path)? {
            Some(uri) => uri,
            None => self
                .find(&format!("{path}.old"))?
                .ok_or(Error::NotFound("repository file"))?,
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

    /// Writes a new file at `path`, whole: beside it first, then renamed.
    fn write(&self, path: &str, bytes: &[u8]) -> Result<()> {
        let name = path.rsplit('/').next().unwrap_or(path);
        let partial = format!("{path}.partial");
        let made = call::<Found>("create", self.args(&partial, ""))?;
        let uri = made
            .uri
            .ok_or_else(|| Error::Io(std::io::Error::other("the folder made no file")))?;
        {
            let mut file = Self::open(&uri, true)?;
            file.write_all(bytes)?;
            file.sync_all()?;
        }
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
        Ok(())
    }

    fn delete_file(&self, path: &str) -> Result<()> {
        self.known().remove(path);
        call::<serde_json::Value>("delete", self.args(path, "")).map(drop)
    }
}

impl Target for DocumentTarget {
    fn put(&self, path: &str, bytes: &[u8]) -> Result<()> {
        let at = self.at(path)?;
        self.write(&at, bytes)?;
        // the repository is made: say what the folder holds
        if path == "config" {
            self.write(&format!("{}README.txt", self.base), self.readme.as_bytes())?;
        }
        Ok(())
    }

    fn get(&self, path: &str) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        self.readable(&self.at(path)?)?.read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    fn get_range(&self, path: &str, offset: u64, len: u64) -> Result<Vec<u8>> {
        let mut file = self.readable(&self.at(path)?)?;
        file.seek(SeekFrom::Start(offset))?;
        let mut bytes = Vec::new();
        file.take(len).read_to_end(&mut bytes)?;
        if bytes.len() as u64 != len {
            return Err(Error::Corrupt(format!(
                "{path} is shorter than its index says"
            )));
        }
        Ok(bytes)
    }

    fn list(&self, prefix: &str) -> Result<Vec<Listed>> {
        let folder = prefix.trim_end_matches('/');
        let from = if folder.is_empty() {
            self.base.clone()
        } else {
            self.at(folder)?
        };
        let files = call::<Files>("list", self.args(&from, ""))?.files;
        let mut out: Vec<Listed> = files
            .into_iter()
            .filter_map(|f| {
                let path = f.path.strip_prefix(&self.base)?.to_owned();
                Some(Listed { path, size: f.size })
            })
            .collect();
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    fn delete(&self, path: &str) -> Result<()> {
        self.delete_file(&self.at(path)?)
    }
}
