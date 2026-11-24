// SPDX-License-Identifier: AGPL-3.0-or-later

//! A sync round with a server: push the outbox, then pull until caught up.
//! The network is behind [`Remote`], so the apps bring their own HTTP and
//! the tests a server in memory. The store is only locked to read a batch
//! or take in a page, never while waiting on the network, so typing never
//! waits on a slow connection.

use std::sync::Mutex;
use std::time::Duration;

use app_protocol::sync::{HeavyDocs, PullAnswer, PushAnswer, PushRequest};

use super::files::{FileError, FilePart};
use crate::error::Error;
use crate::store::{Store, Taken};
use app_protocol::files::{FileState, FilesInfo};

/// Changes per push, and about how many bytes. Well under the server's
/// default request limit of 16 MB once in base64.
pub const PUSH_CHANGES: usize = 500;
/// See [`PUSH_CHANGES`].
pub const PUSH_BYTES: usize = 4 * 1024 * 1024;
/// Changes per pull.
pub const PULL_PAGE: usize = 500;

/// Why the server could not be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RemoteError {
    /// No answer: no network, server down, timeout. Worth retrying.
    #[error("cannot reach the server: {0}")]
    Offline(String),
    /// The session is gone (signed out elsewhere, revoked, expired). A new
    /// sign-in is needed, retrying will not help.
    #[error("signed out")]
    SignedOut,
    /// The server answered with an error.
    #[error("the server refused: {0}")]
    Refused(String),
}

/// The server, as a sync round needs it: the account's space there.
pub trait Remote {
    /// Sends changes.
    ///
    /// # Errors
    ///
    /// See [`RemoteError`].
    fn push(&mut self, request: &PushRequest) -> Result<PushAnswer, RemoteError>;

    /// Changes after `after`, at most `limit`.
    ///
    /// # Errors
    ///
    /// See [`RemoteError`].
    fn pull(&mut self, after: i64, limit: usize) -> Result<PullAnswer, RemoteError>;

    /// Documents with at least `min` changes on the server. A server that
    /// cannot tell has none.
    ///
    /// # Errors
    ///
    /// See [`RemoteError`].
    fn heavy(&mut self, min: i64) -> Result<HeavyDocs, RemoteError> {
        let _ = min;
        Ok(HeavyDocs { docs: Vec::new() })
    }

    /// The account's files policy and usage, `None` from a server that
    /// keeps no files: files stay on the devices then.
    ///
    /// # Errors
    ///
    /// See [`RemoteError`].
    fn files_info(&mut self) -> Result<Option<FilesInfo>, RemoteError> {
        Ok(None)
    }

    /// Where a file stands on the server.
    ///
    /// # Errors
    ///
    /// See [`FileError`].
    fn file_state(&mut self, address: &str) -> Result<FileState, FileError> {
        let _ = address;
        Err(FileError::Missing)
    }

    /// Sends one part of a file.
    ///
    /// # Errors
    ///
    /// See [`FileError`].
    fn put_file(&mut self, address: &str, part: FilePart<'_>) -> Result<FileState, FileError> {
        let _ = (address, part);
        Err(FileError::Missing)
    }

    /// The content of a file, as the server holds it (sealed for e2ee).
    ///
    /// # Errors
    ///
    /// See [`FileError`].
    fn get_file(&mut self, address: &str) -> Result<Box<dyn std::io::Read + Send>, FileError> {
        let _ = address;
        Err(FileError::Missing)
    }

    /// Takes back a file the space let go and the server still keeps.
    ///
    /// # Errors
    ///
    /// See [`FileError`].
    fn keep_file(&mut self, address: &str) -> Result<(), FileError> {
        let _ = address;
        Err(FileError::Missing)
    }

    /// Lets a file go, if the space's log is still at `head`. `Ok(false)`
    /// when it moved on.
    ///
    /// # Errors
    ///
    /// See [`FileError`].
    fn release_file(&mut self, address: &str, head: i64) -> Result<bool, FileError> {
        let _ = (address, head);
        Err(FileError::Missing)
    }
}

/// Changes of one document on the server past which it is worth replacing
/// them with the whole document.
pub const COMPACT_AFTER: i64 = 200;

/// Queues whole documents for those the server holds many changes of, the
/// next round sends them and the server empties what they replace. Run
/// after a round that ended well, now and then. Returns how many were
/// queued.
///
/// # Errors
///
/// See [`SyncError`].
pub fn compact(store: &impl Access, remote: &mut impl Remote, min: i64) -> Result<u32, SyncError> {
    // notes deleted for good long ago go first, as empty documents
    let mut queued = store.with(Store::sync_forget_deleted)?;
    for doc in remote.heavy(min)?.docs {
        if store.with(|s| s.sync_queue_compaction(&doc.doc))? {
            queued += 1;
        }
    }
    Ok(queued)
}

/// Short access to the store, for one step of a round.
pub trait Access {
    /// Runs `f` with the store.
    ///
    /// # Errors
    ///
    /// What `f` returns, or a store that is closed.
    fn with<T>(&self, f: impl FnOnce(&mut Store) -> crate::Result<T>) -> crate::Result<T>;
}

impl Access for Mutex<Store> {
    fn with<T>(&self, f: impl FnOnce(&mut Store) -> crate::Result<T>) -> crate::Result<T> {
        f(&mut self
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner))
    }
}

/// Why a round stopped.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    /// The server.
    #[error(transparent)]
    Remote(#[from] RemoteError),
    /// The vault.
    #[error(transparent)]
    Local(#[from] Error),
}

/// What a round did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Round {
    /// Changes sent.
    pub pushed: u32,
    /// What came in.
    pub taken: Taken,
    /// The server had lost changes, everything was sent again.
    pub resent: bool,
}

/// Pushes everything waiting, then pulls until the server has nothing
/// more. Safe to stop anywhere: an unacknowledged push is sent again and
/// kept once, a page not taken in is pulled again.
///
/// # Errors
///
/// The first failure, see [`SyncError`]. What was done before it stays.
pub fn run(store: &impl Access, remote: &mut impl Remote) -> Result<Round, SyncError> {
    let mut round = Round::default();
    let mut resent = false;
    'round: loop {
        while let Some((request, up_to)) =
            store.with(|s| s.sync_push_batch(PUSH_CHANGES, PUSH_BYTES))?
        {
            remote.push(&request)?;
            store.with(|s| s.sync_ack(up_to))?;
            round.pushed += u32::try_from(request.changes.len()).unwrap_or(u32::MAX);
        }
        loop {
            let (cursor, cursor_id) = store.with(|s| s.sync_cursor())?;
            let page = remote.pull(cursor, PULL_PAGE)?;
            let diverged = cursor > 0
                && (page.head < cursor || (cursor_id.is_some() && page.anchor != cursor_id));
            if diverged && !resent {
                // the server's log is not the one this device followed: it
                // was restored from an older backup or started over. What
                // it lost comes back from every device that still has it
                store.with(Store::sync_resend_all)?;
                resent = true;
                round.resent = true;
                continue 'round;
            }
            let taken = store.with(|s| s.sync_take_page(&page))?;
            round.taken.applied += taken.applied;
            round.taken.own += taken.own;
            round.taken.rejected += taken.rejected;
            if !page.more || page.changes.is_empty() {
                break 'round;
            }
        }
    }
    Ok(round)
}

/// Waits between failed rounds: doubling from 2 seconds up to 5 minutes
/// (or a lower cap), each wait drawn from its upper half so devices that
/// lost the server together do not all come back in the same second.
#[derive(Debug, Clone)]
pub struct Backoff {
    failures: u32,
    most: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            failures: 0,
            most: Self::MOST,
        }
    }
}

impl Backoff {
    const FIRST: Duration = Duration::from_secs(2);
    const MOST: Duration = Duration::from_mins(5);

    /// Waits that stop growing at `most`.
    pub fn capped(most: Duration) -> Self {
        Self { failures: 0, most }
    }

    /// The wait after one more failure. `random` is any random number.
    pub fn failed(&mut self, random: u64) -> Duration {
        let full = Self::FIRST
            .saturating_mul(1 << self.failures.min(16))
            .min(self.most);
        self.failures = self.failures.saturating_add(1);
        let half = full / 2;
        let half_ms = u64::try_from(half.as_millis()).unwrap_or(u64::MAX).max(1);
        half + Duration::from_millis(random % half_ms)
    }

    /// Back to short waits after a round that went well.
    pub fn succeeded(&mut self) {
        self.failures = 0;
    }

    /// Failures in a row.
    pub fn failures(&self) -> u32 {
        self.failures
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_grow_with_jitter_and_stop_at_five_minutes() {
        let mut backoff = Backoff::default();
        let first = backoff.failed(0);
        assert_eq!(first, Duration::from_secs(1));
        let second = backoff.failed(u64::MAX);
        assert!(second >= Duration::from_secs(2) && second < Duration::from_secs(4));
        for _ in 0..40 {
            let wait = backoff.failed(12345);
            assert!(wait <= Duration::from_secs(300), "{wait:?}");
            assert!(wait >= Duration::from_secs(1));
        }
        assert!(backoff.failed(0) >= Duration::from_secs(150));
        backoff.succeeded();
        assert!(backoff.failed(7) < Duration::from_secs(2));
    }

    #[test]
    fn a_capped_backoff_stops_at_its_cap() {
        let mut backoff = Backoff::capped(Duration::from_mins(1));
        for _ in 0..30 {
            assert!(backoff.failed(u64::MAX) <= Duration::from_mins(1));
        }
    }
}
