// SPDX-License-Identifier: AGPL-3.0-or-later

//! Where a vault keeps its files: a folder on this device, or the browser's
//! storage in the web version, which has no folders to open. Paths keep the
//! folder layout either way.

use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::crypto::random_bytes;
use crate::error::{Error, Result};

/// A vault's files. Reading a missing file fails with
/// [`std::io::ErrorKind::NotFound`].
pub trait Disk: Send + Sync + std::fmt::Debug {
    /// Opens a file to read.
    ///
    /// # Errors
    ///
    /// A missing or unreadable file.
    fn open(&self, path: &Path) -> std::io::Result<Box<dyn Read + Send>>;

    /// The size of a file.
    ///
    /// # Errors
    ///
    /// A missing or unreadable file.
    fn len(&self, path: &Path) -> std::io::Result<u64>;

    /// Whether a file or a folder is there.
    fn exists(&self, path: &Path) -> bool;

    /// Makes a folder and those above it, nothing when it is there.
    ///
    /// # Errors
    ///
    /// A folder that cannot be made.
    fn make_folder(&self, dir: &Path) -> std::io::Result<()>;

    /// Writes a file whole: `fill` writes its content, and the file is
    /// there only once `fill` succeeded, never cut short by a crash.
    ///
    /// # Errors
    ///
    /// The error of `fill`, or of writing.
    fn write(&self, path: &Path, fill: &mut dyn FnMut(&mut dyn Write) -> Result<()>) -> Result<()>;

    /// Moves a file or a folder, replacing a file there.
    ///
    /// # Errors
    ///
    /// A missing source, or a failed move.
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()>;

    /// Removes a file, nothing when it is not there.
    ///
    /// # Errors
    ///
    /// A file that cannot be removed.
    fn remove(&self, path: &Path) -> std::io::Result<()>;

    /// Removes a folder with everything in it, nothing when it is not there.
    ///
    /// # Errors
    ///
    /// When something cannot be removed, the things before it are gone.
    fn remove_all(&self, dir: &Path) -> std::io::Result<()>;

    /// What a folder holds, files and folders, empty when it is not there.
    ///
    /// # Errors
    ///
    /// A folder that cannot be read.
    fn list(&self, dir: &Path) -> std::io::Result<Vec<PathBuf>>;

    /// How long ago a file was last written, `None` when it cannot be told.
    fn age(&self, path: &Path) -> Option<Duration>;
}

/// The folders of this device.
#[derive(Debug, Clone, Copy, Default)]
pub struct NativeDisk;

impl Disk for NativeDisk {
    fn open(&self, path: &Path) -> std::io::Result<Box<dyn Read + Send>> {
        Ok(Box::new(File::open(path)?))
    }

    fn len(&self, path: &Path) -> std::io::Result<u64> {
        Ok(std::fs::metadata(path)?.len())
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn make_folder(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)
    }

    fn write(&self, path: &Path, fill: &mut dyn FnMut(&mut dyn Write) -> Result<()>) -> Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| Error::Corrupt("a file path has no folder".into()))?;
        std::fs::create_dir_all(parent)?;
        // written beside, then renamed over: a crash leaves the old file or
        // a stray .tmp, never half a file
        let suffix = crate::blobs::hex(&random_bytes::<6>()?);
        let tmp = path.with_extension(format!("{suffix}.tmp"));
        let written = (|| {
            let mut out = BufWriter::new(File::create(&tmp)?);
            fill(&mut out)?;
            out.into_inner()
                .map_err(|e| Error::Io(e.into_error()))?
                .sync_all()?;
            Ok(())
        })();
        match written {
            Ok(()) => Ok(std::fs::rename(&tmp, path)?),
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                Err(e)
            }
        }
    }

    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        std::fs::rename(from, to)
    }

    fn remove(&self, path: &Path) -> std::io::Result<()> {
        match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    fn remove_all(&self, dir: &Path) -> std::io::Result<()> {
        match std::fs::remove_dir_all(dir) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    fn list(&self, dir: &Path) -> std::io::Result<Vec<PathBuf>> {
        match std::fs::read_dir(dir) {
            Ok(entries) => Ok(entries.filter_map(|e| e.ok().map(|e| e.path())).collect()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(err) => Err(err),
        }
    }

    fn age(&self, path: &Path) -> Option<Duration> {
        std::fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
    }
}
