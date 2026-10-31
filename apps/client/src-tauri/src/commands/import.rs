// SPDX-License-Identifier: AGPL-3.0-or-later

//! Importing from other apps. The page reads the formats, the app opens the
//! export, hands its entries to the page as asked, stores the files straight
//! from the export and writes the notes a batch at a time.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use app_core::id::{FolderId, NoteId};
use app_core::import::source::{Entry, Source};
use app_core::store::{ImportBatch, ImportFile, ImportFolder, ImportNote, ImportPlace};
use serde::{Deserialize, Serialize};
use tauri::State;
use tauri::ipc::{Request, Response};

use super::files::body_bytes;
use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

/// An export being imported.
struct Open {
    source: Source,
    place: Option<ImportPlace>,
}

/// The exports open, by the number the page knows them by.
#[derive(Default)]
pub struct Imports {
    open: Mutex<HashMap<u32, Open>>,
    next: AtomicU32,
}

impl Imports {
    fn with<T>(&self, id: u32, f: impl FnOnce(&mut Open) -> CommandResult<T>) -> CommandResult<T> {
        let mut open = self
            .open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let item = open
            .get_mut(&id)
            .ok_or_else(|| CommandError::new("notFound", "this import is closed"))?;
        f(item)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Opened {
    id: u32,
    /// The export's file or folder name.
    name: String,
    entries: Vec<Entry>,
}

/// Opens a folder, an archive or a file to import from.
#[tauri::command]
pub async fn import_open(
    app: tauri::AppHandle,
    imports: State<'_, Imports>,
    path: String,
) -> CommandResult<Opened> {
    let (source, name) = if crate::picked::is_document(&path) {
        let name = crate::picked::document_name(&path);
        let file = crate::picked::open_document(&app, &path, false)?;
        (Source::open_file(file, &name)?, name)
    } else {
        let path = crate::picked::picked_path(&app, &path)?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        (Source::open(&path)?, name)
    };
    let entries = source.entries().to_vec();
    let id = imports.next.fetch_add(1, Ordering::Relaxed);
    imports
        .open
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(
            id,
            Open {
                source,
                place: None,
            },
        );
    Ok(Opened { id, name, entries })
}

/// One entry of an open export, as raw bytes.
#[tauri::command]
pub async fn import_read(
    imports: State<'_, Imports>,
    id: u32,
    path: String,
) -> CommandResult<Response> {
    imports.with(id, |open| Ok(Response::new(open.source.read(&path)?)))
}

/// The keys among `keys` an earlier import already brought in.
#[tauri::command]
pub async fn import_known(
    state: State<'_, AppState>,
    importer: String,
    keys: Vec<String>,
) -> CommandResult<Vec<String>> {
    state.quietly(|s| Ok(s.store.imported(&importer, &keys)?.into_iter().collect()))
}

/// Makes the import's own folder: everything of this import goes in it.
#[tauri::command]
pub async fn import_begin(
    state: State<'_, AppState>,
    imports: State<'_, Imports>,
    id: u32,
    name: String,
) -> CommandResult<FolderId> {
    imports.with(id, |open| {
        let root = state.with(|s| s.store.create_folder(None, &name))?.id;
        open.place = Some(ImportPlace::new(root));
        Ok(root)
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stored {
    sha256: String,
    size: u64,
}

/// Stores bytes the page decoded (an Evernote picture, a data url) in the
/// vault's files without attaching them yet: the batch names them by hash.
#[tauri::command]
pub async fn import_put(state: State<'_, AppState>, request: Request<'_>) -> CommandResult<Stored> {
    let bytes = body_bytes(&request)?;
    let blobs = state.quietly(|s| Ok(s.blobs.clone()))?;
    Ok(Stored {
        sha256: blobs.put(&bytes)?,
        size: bytes.len() as u64,
    })
}

/// A file of a note as the page names it: an entry of the export, or bytes
/// it stored with [`import_put`].
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageFile {
    name: String,
    mime: String,
    entry: Option<String>,
    sha256: Option<String>,
    size: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageNote {
    #[serde(flatten)]
    note: ImportNote,
    #[serde(default)]
    sources: Vec<PageFile>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageBatch {
    importer: String,
    #[serde(default)]
    folders: Vec<ImportFolder>,
    notes: Vec<PageNote>,
}

/// A file left out, for the report.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeftOut {
    note: String,
    name: String,
    /// `missing`, `tooLarge`, `typeRefused`, `tooManyFiles` or `unreadable`.
    why: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Written {
    notes: Vec<(String, NoteId)>,
    left_out: Vec<LeftOut>,
}

/// Writes a batch: its files stored from the export (outside the vault's
/// lock, they can be large), then its folders and notes in one transaction.
#[tauri::command]
pub async fn import_write(
    state: State<'_, AppState>,
    imports: State<'_, Imports>,
    id: u32,
    batch: PageBatch,
) -> CommandResult<Written> {
    let blobs = state.quietly(|s| Ok(s.blobs.clone()))?;
    let policy = state.sync.files_policy();
    imports.with(id, |open| {
        let mut left_out = Vec::new();
        let mut notes = Vec::with_capacity(batch.notes.len());
        for PageNote { mut note, sources } in batch.notes {
            for file in sources {
                let mut leave = |why| {
                    left_out.push(LeftOut {
                        note: note.title.clone(),
                        name: file.name.clone(),
                        why,
                    });
                };
                if let Some(p) = &policy {
                    if p.max_files_per_note > 0
                        && note.files.len() as u64 >= u64::from(p.max_files_per_note)
                    {
                        leave("tooManyFiles");
                        continue;
                    }
                    if !app_protocol::files::allows(p, &file.name, &file.mime) {
                        leave("typeRefused");
                        continue;
                    }
                }
                let stored = match (&file.entry, &file.sha256) {
                    (Some(entry), _) => match open.source.store(entry, &blobs) {
                        Ok(stored) => Some(stored),
                        Err(app_core::Error::NotFound(_)) => {
                            leave("missing");
                            continue;
                        }
                        Err(_) => None,
                    },
                    (None, Some(sha)) if blobs.has(sha).unwrap_or(false) => {
                        Some((sha.clone(), file.size.unwrap_or(0)))
                    }
                    _ => None,
                };
                let Some((sha256, size)) = stored else {
                    leave("unreadable");
                    continue;
                };
                let limit = policy.as_ref().map_or(0, |p| p.max_file_bytes);
                if limit > 0 && size > limit {
                    leave("tooLarge");
                    continue;
                }
                note.files.push(ImportFile {
                    name: file.name,
                    mime: file.mime,
                    size: i64::try_from(size).unwrap_or(i64::MAX),
                    sha256,
                });
            }
            notes.push(note);
        }
        let place = open
            .place
            .as_mut()
            .ok_or_else(|| CommandError::new("invalid", "the import has no folder yet"))?;
        let batch = ImportBatch {
            importer: batch.importer,
            folders: batch.folders,
            notes,
        };
        let notes = state.with(|s| s.store.import_batch(place, &batch))?;
        Ok(Written { notes, left_out })
    })
}

/// Closes an export, removing what opening it wrote to the temporary folder.
#[tauri::command]
pub async fn import_close(imports: State<'_, Imports>, id: u32) -> CommandResult<()> {
    imports
        .open
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&id);
    Ok(())
}
