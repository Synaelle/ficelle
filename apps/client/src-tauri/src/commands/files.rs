// SPDX-License-Identifier: AGPL-3.0-or-later

//! Attachments and markdown export.

use std::path::PathBuf;

use app_core::id::{AttachmentId, NoteId};
use app_core::model::Attachment;
use serde::Serialize;
use tauri::ipc::{InvokeBody, Request};
use tauri::{Manager, State};

use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

/// Largest file the page can hand over: it arrives whole in memory. Larger
/// ones come from other apps' share sheets (read from disk a piece at a
/// time) or not at all.
const MAX_ATTACHMENT_BYTES: usize = 1024 * 1024 * 1024;

/// Whether a file may be attached to a note: the server's rules for this
/// account when it keeps files (size, type, attachments per note), and the
/// size the page can hand over.
fn allowed(state: &AppState, note: NoteId, name: &str, mime: &str, size: u64) -> CommandResult<()> {
    let policy = state.sync.files_policy();
    let too_large = |limit: u64| limit > 0 && size > limit;
    if too_large(policy.as_ref().map_or(0, |p| p.max_file_bytes)) {
        return Err(CommandError::new(
            "tooLarge",
            "this file is larger than allowed",
        ));
    }
    let Some(policy) = policy else {
        return Ok(());
    };
    if !app_protocol::files::allows(&policy, name, mime) {
        return Err(CommandError::new(
            "typeRefused",
            "this type of file is not allowed",
        ));
    }
    if policy.max_files_per_note > 0 {
        let count = state.quietly(|s| s.store.list_attachments(note))?.len();
        if count >= usize::try_from(policy.max_files_per_note).unwrap_or(usize::MAX) {
            return Err(CommandError::new(
                "tooManyFiles",
                "this note has as many files as allowed",
            ));
        }
    }
    Ok(())
}

/// Decodes `%XX` escapes. The ui sends the file name with
/// `encodeURIComponent` because header values must be ASCII.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The bytes a command was sent. Desktop web views hand them over raw, the
/// Android bridge sends them as json, an array of numbers or an object of
/// numbered entries (a typed array once serialized).
pub(crate) fn body_bytes<'a>(
    request: &'a Request<'_>,
) -> CommandResult<std::borrow::Cow<'a, [u8]>> {
    let byte = |v: &serde_json::Value| v.as_u64().and_then(|n| u8::try_from(n).ok());
    let bad = || CommandError::new("invalid", "the body must be bytes");
    match request.body() {
        InvokeBody::Raw(bytes) => Ok(std::borrow::Cow::Borrowed(bytes)),
        InvokeBody::Json(serde_json::Value::Array(items)) => items
            .iter()
            .map(byte)
            .collect::<Option<Vec<u8>>>()
            .map(std::borrow::Cow::Owned)
            .ok_or_else(bad),
        InvokeBody::Json(serde_json::Value::Object(entries)) => {
            let mut bytes = vec![0u8; entries.len()];
            for (key, value) in entries {
                let at: usize = key.parse().map_err(|_| bad())?;
                *bytes.get_mut(at).ok_or_else(bad)? = byte(value).ok_or_else(bad)?;
            }
            Ok(std::borrow::Cow::Owned(bytes))
        }
        InvokeBody::Json(_) => Err(bad()),
    }
}

fn header<'a>(request: &'a Request<'_>, name: &str) -> CommandResult<&'a str> {
    request
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| CommandError::new("invalid", format!("missing {name} header")))
}

/// Stores a file and attaches it to a note. The bytes come as the raw request
/// body, which avoids encoding images as huge JSON arrays. Metadata travels in
/// headers: `x-note-id`, `x-file-name` (percent encoded) and `x-mime-type`.
#[tauri::command]
pub async fn add_attachment(
    state: State<'_, AppState>,
    request: Request<'_>,
) -> CommandResult<Attachment> {
    let bytes = body_bytes(&request)?;
    if bytes.len() > MAX_ATTACHMENT_BYTES {
        return Err(CommandError::new("tooLarge", "this file is too large"));
    }
    let note: NoteId = header(&request, "x-note-id")?
        .parse()
        .map_err(|_| CommandError::new("invalid", "bad note id"))?;
    let name = percent_decode(header(&request, "x-file-name")?)
        .ok_or_else(|| CommandError::new("invalid", "bad file name"))?;
    let mime = header(&request, "x-mime-type")?.to_owned();
    allowed(&state, note, &name, &mime, bytes.len() as u64)?;
    let size = i64::try_from(bytes.len()).unwrap_or(i64::MAX);
    state.with(|s| {
        let sha = s.blobs.put(&bytes)?;
        s.store.add_attachment(note, &name, &mime, size, &sha)
    })
}

/// Folder where `MainActivity` copies files shared from other apps, inside the
/// app's own cache. Only files directly in it can be taken.
const INBOX_DIR: &str = "inbox";

/// Attaches a file shared from another app and deletes the copy. The path
/// comes from the page, so it must point straight into the inbox folder.
#[tauri::command]
pub async fn attach_inbox_file(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    note: NoteId,
    path: PathBuf,
    name: String,
    mime: String,
) -> CommandResult<Attachment> {
    let real = path
        .canonicalize()
        .map_err(|e| CommandError::new("file", e.to_string()))?;
    // the share target's own folder, `cacheDir/inbox`: any other folder
    // named inbox is not it
    let inbox = app
        .path()
        .app_cache_dir()
        .and_then(|dir| Ok(dir.join(INBOX_DIR).canonicalize()?))
        .ok();
    let in_inbox = inbox.is_some_and(|inbox| real.parent() == Some(inbox.as_path()));
    if !in_inbox || !real.is_file() {
        return Err(CommandError::new("invalid", "not a shared file"));
    }
    let size = real
        .metadata()
        .map_err(|e| CommandError::new("file", e.to_string()))?
        .len();
    let checked = allowed(&state, note, &name, &mime, size);
    // stored a piece at a time: a shared video need not fit in memory
    let stored = checked.and_then(|()| {
        state.with(|s| {
            let sha = s.blobs.put_path(&real)?;
            s.store.add_attachment(
                note,
                &name,
                &mime,
                i64::try_from(size).unwrap_or(i64::MAX),
                &sha,
            )
        })
    });
    let _ = std::fs::remove_file(&real);
    stored
}

/// Gives an image attachment its small preview, made by the page. The
/// bytes come as the raw body, the attachment id in `x-attachment-id`.
#[tauri::command]
pub async fn set_attachment_thumb(
    state: State<'_, AppState>,
    request: Request<'_>,
) -> CommandResult<()> {
    let bytes = body_bytes(&request)?;
    let id: AttachmentId = header(&request, "x-attachment-id")?
        .parse()
        .map_err(|_| CommandError::new("invalid", "bad attachment id"))?;
    state.with(|s| {
        let sha = s.blobs.put(&bytes)?;
        s.store.set_attachment_thumb(id, &sha)
    })
}

#[tauri::command]
pub async fn list_attachments(
    state: State<'_, AppState>,
    id: NoteId,
) -> CommandResult<Vec<Attachment>> {
    state.with(|s| s.store.list_attachments(id))
}

/// Every attachment of the notes not in the trash, with its note's title.
#[tauri::command]
pub async fn all_attachments(
    state: State<'_, AppState>,
) -> CommandResult<Vec<app_core::model::NoteAttachment>> {
    state.with(|s| s.store.all_attachments())
}

#[tauri::command]
pub async fn remove_attachment(state: State<'_, AppState>, id: AttachmentId) -> CommandResult<()> {
    state.with(|s| s.store.remove_attachment(id))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSummary {
    notes: usize,
    folders: usize,
    attachments: usize,
    missing_attachments: Vec<String>,
}

impl From<app_core::export::ExportReport> for ExportSummary {
    fn from(report: app_core::export::ExportReport) -> Self {
        Self {
            notes: report.notes,
            folders: report.folders,
            attachments: report.attachments,
            missing_attachments: report.missing_attachments,
        }
    }
}

#[tauri::command]
pub async fn export_markdown(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    dir: String,
) -> CommandResult<ExportSummary> {
    let dir = crate::picked::picked_path(&app, &dir)?;
    state.with(|s| {
        let report = app_core::export::export_markdown(&s.store, &dir, &s.blobs)?;
        Ok(ExportSummary::from(report))
    })
}

#[cfg(test)]
mod tests {
    use super::percent_decode;

    #[test]
    fn decodes_names_from_the_ui() {
        assert_eq!(
            percent_decode("vigne%20%C3%A9t%C3%A9.png").as_deref(),
            Some("vigne été.png")
        );
        assert_eq!(percent_decode("plain.png").as_deref(), Some("plain.png"));
        assert_eq!(percent_decode("bad%zz"), None);
        assert_eq!(percent_decode("cut%4"), None);
    }
}
