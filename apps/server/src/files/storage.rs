// SPDX-License-Identifier: AGPL-3.0-or-later

//! Where file bytes live. Everything above talks to [`Storage`], so another
//! backend (an S3 compatible store) replaces [`LocalDisk`] without changing
//! anything else. Calls block: callers run them off the async runtime.
//!
//! An upload is written to an incoming object named after its space and
//! address, then moved into place under its object name once complete.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

/// A file backend.
pub trait Storage: Send + Sync {
    /// Writes bytes of an upload at `offset`. Bytes past it are dropped
    /// first, so a part sent again replaces what came before.
    ///
    /// # Errors
    ///
    /// Backend errors.
    fn write_incoming(&self, upload: &str, offset: u64, bytes: &[u8]) -> std::io::Result<()>;

    /// Reads an upload from the start.
    ///
    /// # Errors
    ///
    /// Backend errors, `NotFound` without that upload.
    fn read_incoming(&self, upload: &str) -> std::io::Result<Box<dyn Read + Send>>;

    /// Moves a complete upload into place. When the object exists already
    /// the upload is dropped instead.
    ///
    /// # Errors
    ///
    /// Backend errors.
    fn complete(&self, upload: &str, object: &str) -> std::io::Result<()>;

    /// Drops an upload, done or abandoned.
    ///
    /// # Errors
    ///
    /// Backend errors other than the upload being gone already.
    fn discard(&self, upload: &str) -> std::io::Result<()>;

    /// Reads an object from `offset`.
    ///
    /// # Errors
    ///
    /// Backend errors, `NotFound` without that object.
    fn read(&self, object: &str, offset: u64) -> std::io::Result<Box<dyn Read + Send>>;

    /// Deletes an object.
    ///
    /// # Errors
    ///
    /// Backend errors other than the object being gone already.
    fn delete(&self, object: &str) -> std::io::Result<()>;

    /// Every upload under way, to sweep the abandoned ones.
    ///
    /// # Errors
    ///
    /// Backend errors.
    fn incoming(&self) -> std::io::Result<Vec<String>>;
}

/// Object and upload names are made by the server (hex, letters, digits,
/// `-` and `_`), never taken from a request as they are. This keeps any
/// other name out of the file system.
fn safe(name: &str) -> std::io::Result<&str> {
    let ok = !name.is_empty()
        && name.len() <= 160
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if ok {
        Ok(name)
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "bad object name",
        ))
    }
}

fn gone_is_fine(result: std::io::Result<()>) -> std::io::Result<()> {
    match result {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// Files in a folder: `objects/ab/<name>` and `incoming/<upload>`.
pub struct LocalDisk {
    root: PathBuf,
}

impl LocalDisk {
    /// A backend under `root`, made on first write.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn object_path(&self, object: &str) -> std::io::Result<PathBuf> {
        let name = safe(object)?;
        let shard = name.get(..2).unwrap_or(name);
        Ok(self.root.join("objects").join(shard).join(name))
    }

    fn incoming_path(&self, upload: &str) -> std::io::Result<PathBuf> {
        Ok(self.root.join("incoming").join(safe(upload)?))
    }
}

impl Storage for LocalDisk {
    fn write_incoming(&self, upload: &str, offset: u64, bytes: &[u8]) -> std::io::Result<()> {
        let path = self.incoming_path(upload)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)?;
        file.set_len(offset)?;
        file.seek(SeekFrom::Start(offset))?;
        file.write_all(bytes)?;
        file.sync_data()
    }

    fn read_incoming(&self, upload: &str) -> std::io::Result<Box<dyn Read + Send>> {
        let file = std::fs::File::open(self.incoming_path(upload)?)?;
        Ok(Box::new(std::io::BufReader::new(file)))
    }

    fn complete(&self, upload: &str, object: &str) -> std::io::Result<()> {
        let from = self.incoming_path(upload)?;
        let to = self.object_path(object)?;
        if to.exists() {
            return gone_is_fine(std::fs::remove_file(from));
        }
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(from, to)
    }

    fn discard(&self, upload: &str) -> std::io::Result<()> {
        gone_is_fine(std::fs::remove_file(self.incoming_path(upload)?))
    }

    fn read(&self, object: &str, offset: u64) -> std::io::Result<Box<dyn Read + Send>> {
        let mut file = std::fs::File::open(self.object_path(object)?)?;
        file.seek(SeekFrom::Start(offset))?;
        Ok(Box::new(std::io::BufReader::new(file)))
    }

    fn delete(&self, object: &str) -> std::io::Result<()> {
        gone_is_fine(std::fs::remove_file(self.object_path(object)?))
    }

    fn incoming(&self) -> std::io::Result<Vec<String>> {
        match std::fs::read_dir(self.root.join("incoming")) {
            Ok(entries) => Ok(entries
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }
}

/// Everything in memory, for tests and a server started without a data
/// folder.
#[derive(Default)]
pub struct Memory {
    objects: Mutex<HashMap<String, Vec<u8>>>,
    uploads: Mutex<HashMap<String, Vec<u8>>>,
}

fn not_found() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::NotFound, "no such file")
}

impl Storage for Memory {
    fn write_incoming(&self, upload: &str, offset: u64, bytes: &[u8]) -> std::io::Result<()> {
        let mut uploads = self.uploads.lock().unwrap_or_else(PoisonError::into_inner);
        let data = uploads.entry(safe(upload)?.to_owned()).or_default();
        data.resize(usize::try_from(offset).unwrap_or(usize::MAX), 0);
        data.extend_from_slice(bytes);
        Ok(())
    }

    fn read_incoming(&self, upload: &str) -> std::io::Result<Box<dyn Read + Send>> {
        let uploads = self.uploads.lock().unwrap_or_else(PoisonError::into_inner);
        let data = uploads.get(upload).ok_or_else(not_found)?.clone();
        Ok(Box::new(std::io::Cursor::new(data)))
    }

    fn complete(&self, upload: &str, object: &str) -> std::io::Result<()> {
        let data = self
            .uploads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(upload)
            .ok_or_else(not_found)?;
        self.objects
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(safe(object)?.to_owned())
            .or_insert(data);
        Ok(())
    }

    fn discard(&self, upload: &str) -> std::io::Result<()> {
        self.uploads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(upload);
        Ok(())
    }

    fn read(&self, object: &str, offset: u64) -> std::io::Result<Box<dyn Read + Send>> {
        let objects = self.objects.lock().unwrap_or_else(PoisonError::into_inner);
        let data = objects.get(object).ok_or_else(not_found)?;
        let from = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(data.len());
        Ok(Box::new(std::io::Cursor::new(data[from..].to_vec())))
    }

    fn delete(&self, object: &str) -> std::io::Result<()> {
        self.objects
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(object);
        Ok(())
    }

    fn incoming(&self) -> std::io::Result<Vec<String>> {
        Ok(self
            .uploads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .keys()
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backends() -> (tempfile::TempDir, Vec<Box<dyn Storage>>) {
        let dir = tempfile::tempdir().unwrap();
        let disk = LocalDisk::new(dir.path().join("files"));
        (dir, vec![Box::new(disk), Box::new(Memory::default())])
    }

    fn read_all(mut r: Box<dyn Read + Send>) -> Vec<u8> {
        let mut out = Vec::new();
        r.read_to_end(&mut out).unwrap();
        out
    }

    #[test]
    fn uploads_land_as_objects() {
        let (_dir, backends) = backends();
        for b in backends {
            b.write_incoming("up1", 0, b"hello ").unwrap();
            b.write_incoming("up1", 6, b"there").unwrap();
            // a part sent again replaces what followed
            b.write_incoming("up1", 6, b"world").unwrap();
            assert_eq!(read_all(b.read_incoming("up1").unwrap()), b"hello world");
            assert_eq!(b.incoming().unwrap(), vec!["up1".to_owned()]);
            b.complete("up1", "abcdef").unwrap();
            assert!(b.incoming().unwrap().is_empty());
            assert_eq!(read_all(b.read("abcdef", 6).unwrap()), b"world");
            // the same object again keeps the first
            b.write_incoming("up2", 0, b"other").unwrap();
            b.complete("up2", "abcdef").unwrap();
            assert_eq!(read_all(b.read("abcdef", 0).unwrap()), b"hello world");
            b.delete("abcdef").unwrap();
            b.delete("abcdef").unwrap();
            assert!(b.read("abcdef", 0).is_err());
        }
    }

    #[test]
    fn names_from_outside_are_refused() {
        let (_dir, backends) = backends();
        for b in backends {
            for bad in ["../x", "a/b", "", "a.b", "a\\b"] {
                assert!(b.write_incoming(bad, 0, b"x").is_err(), "{bad}");
                assert!(b.read(bad, 0).is_err(), "{bad}");
            }
        }
    }
}
