// SPDX-License-Identifier: AGPL-3.0-or-later

//! The open vault, shared by every command.

use std::path::PathBuf;
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
}

impl AppState {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            session: Mutex::new(None),
            sync: crate::sync::Sync::default(),
        }
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
