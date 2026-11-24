// SPDX-License-Identifier: AGPL-3.0-or-later

//! The databases SQLite keeps in memory in the browser, for the web core
//! when it runs without the browser's private storage: the preview of
//! development and the tests. The core forbids unsafe code, and SQLite's
//! memory storage is only reached through an unsafe call, so that call
//! lives here on its own.

#![cfg(target_family = "wasm")]

pub use sqlite_wasm_rs::vfs::memvfs::{MemVfsError, MemVfsUtil};

/// The memory storage SQLite opens databases in when no other is installed,
/// set up by SQLite itself when it starts.
///
/// # Errors
///
/// SQLite could not start or has no memory storage.
pub fn memory_files() -> Result<MemVfsUtil, MemVfsError> {
    // SAFETY: WebAssembly here has a single thread, so this runs on the one
    // that installed the storage and uses SQLite, with nothing registering
    // another storage at the same time
    unsafe { MemVfsUtil::get() }
}
