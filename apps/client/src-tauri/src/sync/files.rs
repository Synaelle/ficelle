// SPDX-License-Identifier: AGPL-3.0-or-later

//! Attachment files with the server: the pass after each round (see
//! [`app_core::sync::files`]), a file fetched when a note needs it, saving
//! a copy, and what the ui shows of the account's files policy.

use std::time::{Duration, Instant};

use app_core::id::NoteId;
use app_core::model::Attachment;
use app_core::sync::engine::{RemoteError, SyncError};
use app_core::sync::files::{FileError, FilePart};
use app_protocol::files::{FileState, FilesInfo, encode_name};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use super::http::{Answer, HttpError};
use super::{Vault, wire};
use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

/// Files up to this size come with the rounds unless the ui says otherwise.
const AUTO_FETCH: u64 = 10 * 1024 * 1024;
/// Removed files are cleaned up here this often.
const CLEAN_EVERY: Duration = Duration::from_hours(6);
/// How long a removed file is kept here without a server to say.
const KEEP_DAYS: u32 = 30;

/// A file call's failure, sorted for the transfer: the network as for the
/// notes, the file missing, or refused with the server's code.
pub(super) fn file_error(err: HttpError) -> FileError {
    match err {
        HttpError::Offline(why) | HttpError::Certificate(why) => {
            FileError::Remote(RemoteError::Offline(why))
        }
        HttpError::Api { status: 401, .. } => FileError::Remote(RemoteError::SignedOut),
        HttpError::Api { status: 404, .. } => FileError::Missing,
        HttpError::Api { status, body } if status >= 500 || status == 429 => {
            FileError::Remote(RemoteError::Offline(format!("{status} {}", body.error)))
        }
        HttpError::Api { body, .. } => FileError::Refused(body.error),
    }
}

pub(super) fn answer(result: Result<Answer, HttpError>) -> Result<FileState, FileError> {
    result
        .map_err(file_error)?
        .parse()
        .map_err(|e| FileError::Refused(e.to_string()))
}

pub(super) fn part_headers(part: &FilePart<'_>) -> Vec<(&'static str, String)> {
    let range = if part.total == 0 {
        "bytes */0".to_owned()
    } else {
        format!(
            "bytes {}-{}/{}",
            part.start,
            part.start + part.bytes.len() as u64 - 1,
            part.total
        )
    };
    let mut headers = vec![("content-range", range)];
    if let Some((name, mime)) = part.declared {
        headers.push(("x-file-name", encode_name(name)));
        headers.push(("x-file-type", mime.to_owned()));
    }
    headers
}

/// A pass at least this often, files a round of the notes did not ask for.
const PASS_EVERY: Duration = Duration::from_mins(5);
/// After a pass that failed (offline, signed out), the next one waits.
const AFTER_FAILURE: Duration = Duration::from_secs(20);

/// The files thread: a pass after each round of the notes (and every few
/// minutes), on its own, so a large file never holds up the notes. Uploads
/// resume where they stopped, so a pass cut off loses little.
pub(super) fn run(sync: &super::Sync, app: &AppHandle) {
    loop {
        {
            let mut shared = sync.lock();
            let since = Instant::now();
            while !shared.files_wanted && since.elapsed() < PASS_EVERY {
                shared = sync
                    .shared
                    .1
                    .wait_timeout(shared, PASS_EVERY)
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .0;
            }
            shared.files_wanted = false;
        }
        let state = app.state::<AppState>();
        let linked = state
            .quietly(|s| Ok(s.store.sync_link()?.is_some_and(|l| l.tokens.is_some())))
            .unwrap_or(false);
        if !linked {
            continue;
        }
        let done = wire(&state).and_then(|(mut remote, _)| after_round(&state, &mut remote));
        match done {
            Ok(fetched) => {
                if fetched {
                    let _ = app.emit("files-changed", ());
                }
            }
            Err(err) => {
                eprintln!("files: {err}");
                std::thread::sleep(AFTER_FAILURE);
            }
        }
    }
}

/// The files pass, and now and then the clean up of files removed long
/// enough ago. Returns whether files arrived.
fn after_round(state: &AppState, wire: &mut super::Wire<'_>) -> Result<bool, SyncError> {
    let blobs = state
        .quietly(|s| Ok(s.blobs.clone()))
        .map_err(|_| RemoteError::Offline("the vault is locked".into()))?;
    let limit = state.sync.lock().auto_fetch.unwrap_or(AUTO_FETCH);
    let done = app_core::sync::files::transfer(&Vault(state), &blobs, wire, limit)?;
    let clean = {
        let mut shared = state.sync.lock();
        let due = shared.cleaned.is_none_or(|t| t.elapsed() >= CLEAN_EVERY);
        if due {
            shared.cleaned = Some(Instant::now());
        }
        due
    };
    if clean {
        clean_up(state);
    }
    Ok(done.fetched > 0)
}

/// Forgets attachments removed longer ago than the account keeps them, then
/// the files nothing refers to any more.
fn clean_up(state: &AppState) {
    let days = state
        .sync
        .lock()
        .files
        .as_ref()
        .map_or(KEEP_DAYS, |info| info.policy.retention_days);
    let keep = i64::from(days) * 24 * 60 * 60 * 1000;
    let result = state.quietly(|s| {
        s.store.forget_removed_files(keep)?;
        let used = s.store.referenced_blobs()?;
        s.blobs.remove_unreferenced(&used)
    });
    if let Err(err) = result {
        eprintln!("cleaning up files: {err:?}");
    }
}

/// An attachment with where its file stands here.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentFile {
    #[serde(flatten)]
    attachment: Attachment,
    /// The file is on this device.
    here: bool,
    /// The server refused it: an error code such as `fileTooLarge`.
    refused: Option<String>,
}

/// A note's attachments, with whether each file is here and whether the
/// server refused it.
#[tauri::command]
pub async fn attachment_files(
    state: State<'_, AppState>,
    note: NoteId,
) -> CommandResult<Vec<AttachmentFile>> {
    state.quietly(|s| {
        let refused = s.store.files_refused()?;
        s.store
            .list_attachments(note)?
            .into_iter()
            .map(|attachment| {
                Ok(AttachmentFile {
                    here: s.blobs.has(&attachment.sha256)?,
                    refused: refused
                        .iter()
                        .find(|(sha, _)| *sha == attachment.sha256)
                        .map(|(_, code)| code.clone()),
                    attachment,
                })
            })
            .collect()
    })
}

/// Fetches a file a note uses from the server, now. Tells the ui with
/// `files-changed` when it is here.
#[tauri::command]
pub async fn fetch_attachment(app: AppHandle, sha256: String) -> CommandResult<()> {
    let worker = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = worker.state::<AppState>();
        let blobs = state.quietly(|s| Ok(s.blobs.clone()))?;
        if blobs.has(&sha256).unwrap_or(false) {
            return Ok(());
        }
        let (mut remote, _) =
            wire(&state).map_err(|e| CommandError::new("offline", e.to_string()))?;
        app_core::sync::files::fetch(&Vault(&state), &blobs, &mut remote, &sha256).map_err(|e| {
            match e {
                FileError::Missing => {
                    CommandError::new("notFound", "the server does not have this file yet")
                }
                FileError::Remote(RemoteError::SignedOut) => {
                    CommandError::new("signedOut", "sign in again")
                }
                FileError::Remote(other) => CommandError::new("offline", other.to_string()),
                FileError::Refused(why) => CommandError::new("refused", why),
            }
        })
    })
    .await
    .map_err(|e| CommandError::new("internal", e.to_string()))??;
    let _ = app.emit("files-changed", ());
    Ok(())
}

/// Writes a copy of an attachment's file where the person chose, read and
/// written a piece at a time.
#[tauri::command]
pub async fn save_attachment(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    sha256: String,
    path: String,
) -> CommandResult<()> {
    let reader = state.quietly(|s| s.blobs.open(&sha256))?;
    let mut reader = reader
        .ok_or_else(|| CommandError::new("notFound", "the file is not on this device yet"))?;
    // an Android document: written in place, it has no folder beside it
    if crate::picked::is_document(&path) {
        let mut out = crate::picked::open_document(&app, &path, true)?;
        return tauri::async_runtime::spawn_blocking(move || std::io::copy(&mut reader, &mut out))
            .await
            .map_err(|e| CommandError::new("internal", e.to_string()))?
            .map(|_| ())
            .map_err(|e| CommandError::new("file", e.to_string()));
    }
    let path = crate::picked::picked_path(&app, &path)?;
    tauri::async_runtime::spawn_blocking(move || {
        let tmp = path.with_extension("part");
        let mut out = std::fs::File::create(&tmp)?;
        std::io::copy(&mut reader, &mut out)?;
        out.sync_all()?;
        drop(out);
        std::fs::rename(&tmp, &path)
    })
    .await
    .map_err(|e| CommandError::new("internal", e.to_string()))?
    .map_err(|e| CommandError::new("file", e.to_string()))
}

/// What the server allows this account and what it uses, as the last round
/// heard. `None` without a server that keeps files.
#[tauri::command]
pub async fn files_info(state: State<'_, AppState>) -> CommandResult<Option<FilesInfo>> {
    Ok(state.sync.lock().files.clone())
}

/// A file the server refused, for the settings.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefusedFile {
    name: String,
    note: String,
    reason: String,
}

/// Attachments the server refused, with their note and the reason.
#[tauri::command]
pub async fn files_refused(state: State<'_, AppState>) -> CommandResult<Vec<RefusedFile>> {
    state.quietly(|s| {
        Ok(s.store
            .refused_attachments()?
            .into_iter()
            .map(|(name, note, reason)| RefusedFile { name, note, reason })
            .collect())
    })
}

/// Files up to `mb` megabytes come with each round, larger ones when opened.
#[tauri::command]
pub async fn files_set_auto_fetch(state: State<'_, AppState>, mb: u32) -> CommandResult<()> {
    state.sync.lock().auto_fetch = Some(u64::from(mb) * 1024 * 1024);
    state.sync.kick();
    Ok(())
}
