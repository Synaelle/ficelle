// SPDX-License-Identifier: AGPL-3.0-or-later

//! Where a repository lives: a few calls any storage can answer, so the
//! repository never knows whether it writes to a folder, S3 or `WebDAV`.
//! Paths are relative, with `/`, and made only by the repository.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// A file the target holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// Its path, as given to [`Target::put`].
    pub path: String,
    /// Its size in bytes.
    pub size: u64,
}

/// A place to keep a repository. A `put` is whole or not at all: a target
/// that cannot promise it writes beside and renames.
pub trait Target: Send + Sync {
    /// Writes `bytes` at `path`, replacing what was there.
    ///
    /// # Errors
    ///
    /// The target refused or could not be reached.
    fn put(&self, path: &str, bytes: &[u8]) -> Result<()>;

    /// Reads the file at `path`.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`] when there is none, else as [`Target::put`].
    fn get(&self, path: &str) -> Result<Vec<u8>>;

    /// Reads `len` bytes of the file at `path` from `offset`.
    ///
    /// # Errors
    ///
    /// As [`Target::get`], and [`Error::Corrupt`] when the file is shorter.
    fn get_range(&self, path: &str, offset: u64, len: u64) -> Result<Vec<u8>> {
        let bytes = self.get(path)?;
        let start = usize::try_from(offset).map_err(|_| short(path))?;
        let end = start
            .checked_add(usize::try_from(len).map_err(|_| short(path))?)
            .ok_or_else(|| short(path))?;
        bytes
            .get(start..end)
            .map(<[u8]>::to_vec)
            .ok_or_else(|| short(path))
    }

    /// The files whose path starts with `prefix` (a folder, ending in `/`),
    /// at any depth.
    ///
    /// # Errors
    ///
    /// As [`Target::put`].
    fn list(&self, prefix: &str) -> Result<Vec<Listed>>;

    /// Removes the file at `path`. Removing one that is not there is fine.
    ///
    /// # Errors
    ///
    /// As [`Target::put`].
    fn delete(&self, path: &str) -> Result<()>;
}

fn short(path: &str) -> Error {
    Error::Corrupt(format!("{path} is shorter than its index says"))
}

/// Whether a path made by the repository is one a target may use: no
/// climbing out, no absolute path, only plain names.
pub fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && path.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        })
}

/// A folder on this device (or a drive, a share mounted as one).
#[derive(Debug, Clone)]
pub struct FolderTarget {
    root: PathBuf,
}

impl FolderTarget {
    /// A target in `root`, created when the first file is written.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn at(&self, path: &str) -> Result<PathBuf> {
        if !safe_path(path) {
            return Err(Error::Invalid(format!("not a repository path: {path}")));
        }
        Ok(self.root.join(path))
    }
}

fn walk(dir: &Path, base: &Path, out: &mut Vec<Listed>) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    for entry in entries {
        let entry = entry?;
        let kind = entry.file_type()?;
        let path = entry.path();
        if kind.is_dir() {
            walk(&path, base, out)?;
        } else if kind.is_file() {
            let name = entry.file_name();
            // a write that never finished
            if name.to_string_lossy().ends_with(".partial") {
                continue;
            }
            let relative = path
                .strip_prefix(base)
                .map_err(|_| Error::Corrupt("a file outside the repository".into()))?;
            let parts: Vec<String> = relative
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            out.push(Listed {
                path: parts.join("/"),
                size: entry.metadata()?.len(),
            });
        }
    }
    Ok(())
}

impl Target for FolderTarget {
    fn put(&self, path: &str, bytes: &[u8]) -> Result<()> {
        let to = self.at(path)?;
        if let Some(dir) = to.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let partial = to.with_extension("partial");
        {
            use std::io::Write;
            let mut file = std::fs::File::create(&partial)?;
            file.write_all(bytes)?;
            // on the disk before it gets its name, a power cut leaves no
            // half file under it
            file.sync_all()?;
        }
        std::fs::rename(&partial, &to).inspect_err(|_| {
            let _ = std::fs::remove_file(&partial);
        })?;
        Ok(())
    }

    fn get(&self, path: &str) -> Result<Vec<u8>> {
        std::fs::read(self.at(path)?).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Error::NotFound("repository file"),
            _ => e.into(),
        })
    }

    fn get_range(&self, path: &str, offset: u64, len: u64) -> Result<Vec<u8>> {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = std::fs::File::open(self.at(path)?).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Error::NotFound("repository file"),
            _ => e.into(),
        })?;
        file.seek(SeekFrom::Start(offset))?;
        let mut bytes = Vec::new();
        file.take(len).read_to_end(&mut bytes)?;
        if bytes.len() as u64 != len {
            return Err(short(path));
        }
        Ok(bytes)
    }

    fn list(&self, prefix: &str) -> Result<Vec<Listed>> {
        let folder = prefix.trim_end_matches('/');
        let dir = if folder.is_empty() {
            self.root.clone()
        } else {
            self.at(folder)?
        };
        let mut out = Vec::new();
        walk(&dir, &self.root, &mut out)?;
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    fn delete(&self, path: &str) -> Result<()> {
        match std::fs::remove_file(self.at(path)?) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

/// A target in memory, for tests (feature `testing`): it can be told to
/// fail.
#[cfg(any(test, feature = "testing"))]
#[derive(Default)]
pub struct MemoryTarget {
    files: std::sync::Mutex<std::collections::BTreeMap<String, Vec<u8>>>,
    /// Fails every call after this many more succeed, while set.
    pub fail_after: std::sync::Mutex<Option<usize>>,
}

#[cfg(any(test, feature = "testing"))]
impl MemoryTarget {
    fn tick(&self) -> Result<()> {
        let mut left = self.fail_after.lock().unwrap();
        match *left {
            Some(0) => Err(Error::Io(std::io::Error::other("the target went away"))),
            Some(n) => {
                *left = Some(n - 1);
                Ok(())
            }
            None => Ok(()),
        }
    }

    /// The files, to look at or damage.
    ///
    /// # Panics
    ///
    /// When a test panicked while holding them.
    pub fn files(&self) -> std::sync::MutexGuard<'_, std::collections::BTreeMap<String, Vec<u8>>> {
        self.files.lock().unwrap()
    }
}

#[cfg(any(test, feature = "testing"))]
impl Target for MemoryTarget {
    fn put(&self, path: &str, bytes: &[u8]) -> Result<()> {
        self.tick()?;
        assert!(safe_path(path), "{path}");
        self.files().insert(path.to_owned(), bytes.to_vec());
        Ok(())
    }

    fn get(&self, path: &str) -> Result<Vec<u8>> {
        self.tick()?;
        self.files()
            .get(path)
            .cloned()
            .ok_or(Error::NotFound("repository file"))
    }

    fn list(&self, prefix: &str) -> Result<Vec<Listed>> {
        self.tick()?;
        Ok(self
            .files()
            .iter()
            .filter(|(path, _)| path.starts_with(prefix))
            .map(|(path, bytes)| Listed {
                path: path.clone(),
                size: bytes.len() as u64,
            })
            .collect())
    }

    fn delete(&self, path: &str) -> Result<()> {
        self.tick()?;
        self.files().remove(path);
        Ok(())
    }
}
