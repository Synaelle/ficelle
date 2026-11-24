// SPDX-License-Identifier: AGPL-3.0-or-later

//! Files on their way to and from the server, for the page: those of the
//! files pass and of each fetch the person asked for, told with the
//! `files-moving` event a few times a second at most. On Android a fetch
//! the person asked for also shows in the system's notifications while it
//! lasts, which keeps the app running until the file is here.

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use app_core::sync::files::{Moving, Progress};
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;

/// The page hears of bytes moving at most this often. A file starting or
/// ending is told at once.
const TELL_EVERY: Duration = Duration::from_millis(250);
/// The system's notification changes at most this often: Android drops
/// updates that come faster.
#[cfg(target_os = "android")]
const NOTIFY_EVERY: Duration = Duration::from_secs(1);

/// What each pass or fetch going on heard last.
#[derive(Default)]
pub struct Transfers {
    inner: Mutex<Heard>,
}

#[derive(Default)]
struct Heard {
    /// By the number of the pass or fetch, with whether the person asked.
    files: BTreeMap<u32, (bool, Vec<Moving>)>,
    next: u32,
    told_at: Option<Instant>,
    /// How many files the page last heard of, so a start or end goes at once.
    told: usize,
    #[cfg(target_os = "android")]
    notified_at: Option<Instant>,
    #[cfg(target_os = "android")]
    notifying: bool,
}

impl Heard {
    /// Every file moving, each once: a file the pass and a fetch both move
    /// shows as the first heard.
    fn merged(&self) -> Vec<Moving> {
        let mut out: Vec<Moving> = Vec::new();
        for file in self.files.values().flat_map(|(_, files)| files) {
            if !out.iter().any(|f| f.sha256 == file.sha256) {
                out.push(file.clone());
            }
        }
        out
    }
}

impl Transfers {
    fn heard(&self) -> std::sync::MutexGuard<'_, Heard> {
        // only progress: a panic mid update leaves numbers the next one corrects
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The files moving now, for the page as it opens.
    pub fn moving(&self) -> Vec<Moving> {
        self.heard().merged()
    }
}

/// A pass or a fetch telling the page how its files go. What it told is
/// taken back when it is dropped, however the call ended.
pub struct Shown<'a> {
    app: &'a AppHandle,
    id: u32,
}

impl<'a> Shown<'a> {
    /// `asked`: a fetch the person asked for, which Android shows in its
    /// notifications.
    pub fn new(app: &'a AppHandle, asked: bool) -> Self {
        let state = app.state::<AppState>();
        let mut heard = state.sync.transfers.heard();
        let id = heard.next;
        heard.next = heard.next.wrapping_add(1);
        heard.files.insert(id, (asked, Vec::new()));
        Self { app, id }
    }

    fn tell(&self, change: impl FnOnce(&mut Heard)) {
        let state = self.app.state::<AppState>();
        let mut heard = state.sync.transfers.heard();
        change(&mut heard);
        let files = heard.merged();
        let due =
            files.len() != heard.told || heard.told_at.is_none_or(|t| t.elapsed() >= TELL_EVERY);
        #[cfg(target_os = "android")]
        notify(&mut heard);
        if !due {
            return;
        }
        heard.told = files.len();
        heard.told_at = Some(Instant::now());
        drop(heard);
        let _ = self.app.emit("files-moving", files);
    }
}

impl Progress for Shown<'_> {
    fn files(&mut self, moving: &[Moving]) {
        let id = self.id;
        self.tell(|heard| {
            if let Some((_, files)) = heard.files.get_mut(&id) {
                files.clear();
                files.extend_from_slice(moving);
            }
        });
    }
}

impl Drop for Shown<'_> {
    fn drop(&mut self) {
        let id = self.id;
        self.tell(|heard| {
            heard.files.remove(&id);
        });
    }
}

/// Shows the first file the person asked for in the system's notification,
/// or ends it when none is left.
#[cfg(target_os = "android")]
fn notify(heard: &mut Heard) {
    let asked = heard
        .files
        .values()
        .filter(|(asked, _)| *asked)
        .find_map(|(_, files)| files.first());
    match asked {
        Some(file) => {
            let due = !heard.notifying
                || file.done >= file.total
                || heard
                    .notified_at
                    .is_none_or(|t| t.elapsed() >= NOTIFY_EVERY);
            if due {
                crate::android_transfer::show(&file.name, file.done, file.total);
                heard.notifying = true;
                heard.notified_at = Some(Instant::now());
            }
        }
        None if heard.notifying => {
            crate::android_transfer::stop();
            heard.notifying = false;
        }
        None => {}
    }
}
