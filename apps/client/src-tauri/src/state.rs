// SPDX-License-Identifier: AGPL-3.0-or-later

//! The open vault, shared by every command.

use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use app_core::blobs::BlobStore;
use app_core::store::Store;
use app_core::vault::Vault;

use crate::error::{CommandError, CommandResult};

/// An unlocked vault.
pub struct Session {
    pub vault: Vault,
    pub store: Store,
    pub blobs: BlobStore,
}

pub struct AppState {
    /// Folder of the vault, inside the app data folder of this identity, so
    /// dev builds never open release data.
    pub dir: PathBuf,
    session: Mutex<Option<Session>>,
    /// The sync thread, woken after each command.
    pub sync: crate::sync::Sync,
    /// Held while the app runs, see [`hold`].
    _holder: Option<File>,
    /// Another copy of the app has the notes.
    elsewhere: bool,
}

/// Locks the vault for this process: two copies of the app on the same notes
/// write over each other's changes, and can leave them impossible to change.
/// The lock file sits beside the vault folder, which a reset deletes. Where
/// the system cannot lock files at all, the app runs without it.
fn hold(dir: &Path) -> Result<Option<File>, ()> {
    let path = dir.with_extension("lock");
    let file = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| {
            File::options()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&path)
        });
    let Ok(file) = file else { return Ok(None) };
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Err(()),
        Err(TryLockError::Error(_)) => Ok(None),
    }
}

impl AppState {
    pub fn new(dir: PathBuf) -> Self {
        let (holder, elsewhere) = match hold(&dir) {
            Ok(holder) => (holder, false),
            Err(()) => (None, true),
        };
        Self {
            dir,
            session: Mutex::new(None),
            sync: crate::sync::Sync::default(),
            _holder: holder,
            elsewhere,
        }
    }

    /// Fails when another copy of the app has the notes open.
    pub fn check_alone(&self) -> CommandResult<()> {
        if self.elsewhere {
            return Err(CommandError::new(
                "openElsewhere",
                "the notes are open in another copy of the app",
            ));
        }
        Ok(())
    }

    pub fn session(&self) -> MutexGuard<'_, Option<Session>> {
        // a poisoned lock means a command panicked, the session data is still sound
        self.session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Runs `f` with the unlocked session, or fails with `locked`. Wakes
    /// the sync thread, which sends what `f` wrote, if anything.
    pub fn with<T>(&self, f: impl FnOnce(&mut Session) -> app_core::Result<T>) -> CommandResult<T> {
        let done = self.quietly(f);
        self.sync.kick();
        done
    }

    /// As [`Self::with`], without waking the sync thread: for the sync
    /// thread itself.
    pub fn quietly<T>(
        &self,
        f: impl FnOnce(&mut Session) -> app_core::Result<T>,
    ) -> CommandResult<T> {
        let mut guard = self.session();
        let session = guard.as_mut().ok_or_else(CommandError::locked)?;
        Ok(f(session)?)
    }

    /// Opens a session from an unlocked store, removes trash past retention
    /// and notes left blank when the app last closed.
    pub fn open(&self, vault: Vault, mut store: Store) -> CommandResult<()> {
        self.check_alone()?;
        store.purge_expired_trash()?;
        store.discard_blank_notes()?;
        let blobs = vault.blobs(&mut store)?;
        *self.session() = Some(Session {
            vault,
            store,
            blobs,
        });
        self.sync.now();
        Ok(())
    }

    pub fn close(&self) {
        *self.session() = None;
    }
}
