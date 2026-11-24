// SPDX-License-Identifier: AGPL-3.0-or-later

//! Importing from other apps, as the core has it
//! (`app_core::commands::import`): the export opened from a picked path or
//! an Android document, its files kept from the cleanup while a batch is
//! written.

use std::collections::BTreeSet;
use std::sync::{Mutex, MutexGuard, PoisonError};

use app_core::commands::import::{Opened, PageBatch, Stored, Written};
use app_core::id::FolderId;
use app_core::import::source::Source;
use tauri::State;
use tauri::ipc::{Request, Response};

use super::files::body_bytes;
use crate::error::CommandResult;
use crate::state::AppState;

/// Files stored for imports while the app runs. A batch refers to its files
/// only once it is written, and the file cleanup in between took them for
/// files nothing refers to. Held while one is stored and while the cleanup
/// runs, so neither sees the other half done.
static PUT: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// The files stored for imports, see [`PUT`].
pub fn put_for_imports() -> MutexGuard<'static, BTreeSet<String>> {
    // a set of names, sound whatever panicked while it was held
    PUT.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The exports open.
pub type Imports = Mutex<app_core::commands::import::Imports>;

fn open(imports: &Imports) -> MutexGuard<'_, app_core::commands::import::Imports> {
    // the exports open, sound whatever panicked while they were held
    imports.lock().unwrap_or_else(PoisonError::into_inner)
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
            .map_or_default(|n| n.to_string_lossy().into_owned());
        (Source::open(&path)?, name)
    };
    Ok(open(&imports).open(source, name))
}

/// One entry of an open export, as raw bytes.
#[tauri::command]
pub async fn import_read(
    imports: State<'_, Imports>,
    id: u32,
    path: String,
) -> CommandResult<Response> {
    Ok(Response::new(open(&imports).read(id, &path)?))
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
    open(&imports).begin(id, || state.with(|s| s.store.begin_import(&name)))
}

/// Stores bytes the page decoded (an Evernote picture, a data url) in the
/// vault's files without attaching them yet: the batch names them by hash.
#[tauri::command]
pub async fn import_put(state: State<'_, AppState>, request: Request<'_>) -> CommandResult<Stored> {
    let bytes = body_bytes(&request)?;
    let blobs = state.quietly(|s| Ok(s.blobs.clone()))?;
    let mut put = put_for_imports();
    let sha256 = blobs.put(&bytes)?;
    put.insert(sha256.clone());
    Ok(Stored {
        sha256,
        size: bytes.len() as u64,
    })
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
    open(&imports).write(
        id,
        batch,
        &blobs,
        policy.as_ref(),
        // kept from the cleanup until the batch refers to it
        |source, entry| {
            let mut put = put_for_imports();
            let stored = source.store(entry, &blobs)?;
            put.insert(stored.0.clone());
            Ok(stored)
        },
        |place, batch| state.with(|s| s.store.import_batch(place, batch)),
    )
}

/// Closes an export, removing what opening it wrote to the temporary folder,
/// and ends the import into its folder.
#[tauri::command]
pub async fn import_close(
    state: State<'_, AppState>,
    imports: State<'_, Imports>,
    id: u32,
) -> CommandResult<()> {
    let root = open(&imports).close(id);
    match root {
        Some(root) => state.quietly(|s| s.store.end_import(root)),
        None => Ok(()),
    }
}
