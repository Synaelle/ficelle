// SPDX-License-Identifier: AGPL-3.0-or-later

//! Notes, folders, tags, trash and history.

use app_core::id::{FolderId, NoteId, TagId, VersionId};
use app_core::model::{Folder, Note, NoteScope, NoteSort, NoteSummary, NoteVersion, Tag};
use app_core::store::{NewNote, NoteChange};
use serde::Deserialize;
use tauri::State;

use crate::error::CommandResult;
use crate::state::AppState;

/// Which notes to list, as sent by the ui.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Scope {
    All,
    Folder { id: Option<FolderId> },
    Tag { id: TagId },
    Trash,
}

impl From<Scope> for NoteScope {
    fn from(scope: Scope) -> Self {
        match scope {
            Scope::All => Self::All,
            Scope::Folder { id } => Self::Folder(id),
            Scope::Tag { id } => Self::Tag(id),
            Scope::Trash => Self::Trash,
        }
    }
}

#[tauri::command]
pub async fn list_notes(
    state: State<'_, AppState>,
    scope: Scope,
    sort: NoteSort,
) -> CommandResult<Vec<NoteSummary>> {
    state.with(|s| s.store.list_notes(scope.into(), sort))
}

#[tauri::command]
pub async fn get_note(state: State<'_, AppState>, id: NoteId) -> CommandResult<Note> {
    state.with(|s| s.store.note(id))
}

#[tauri::command]
pub async fn create_note(state: State<'_, AppState>, note: NewNote) -> CommandResult<Note> {
    state.with(|s| s.store.create_note(&note))
}

#[tauri::command]
pub async fn update_note(
    state: State<'_, AppState>,
    id: NoteId,
    change: NoteChange,
) -> CommandResult<Note> {
    state.with(|s| s.store.update_note(id, change))
}

#[tauri::command]
pub async fn set_pinned(state: State<'_, AppState>, id: NoteId, pinned: bool) -> CommandResult<()> {
    state.with(|s| s.store.set_pinned(id, pinned))
}

#[tauri::command]
pub async fn move_note(
    state: State<'_, AppState>,
    id: NoteId,
    folder: Option<FolderId>,
) -> CommandResult<NoteId> {
    state.with(|s| s.store.move_note(id, folder))
}

#[tauri::command]
pub async fn set_note_tags(
    state: State<'_, AppState>,
    id: NoteId,
    tags: Vec<String>,
) -> CommandResult<()> {
    state.with(|s| s.store.set_note_tags(id, &tags))
}

#[tauri::command]
pub async fn trash_note(state: State<'_, AppState>, id: NoteId) -> CommandResult<()> {
    state.with(|s| s.store.trash_note(id))
}

#[tauri::command]
pub async fn restore_note(state: State<'_, AppState>, id: NoteId) -> CommandResult<()> {
    state.with(|s| s.store.restore_note(id))
}

#[tauri::command]
pub async fn purge_note(state: State<'_, AppState>, id: NoteId) -> CommandResult<()> {
    state.with(|s| s.store.purge_note(id))
}

/// Called when leaving a note, deletes it if it was left blank.
#[tauri::command]
pub async fn discard_if_blank(state: State<'_, AppState>, id: NoteId) -> CommandResult<bool> {
    state.with(|s| s.store.discard_if_blank(id))
}

#[tauri::command]
pub async fn empty_trash(state: State<'_, AppState>) -> CommandResult<usize> {
    state.with(|s| {
        let now = app_core::clock::Clock::now(&app_core::clock::SystemClock);
        s.store.purge_trashed_before(now)
    })
}

#[tauri::command]
pub async fn list_folders(state: State<'_, AppState>) -> CommandResult<Vec<Folder>> {
    state.with(|s| s.store.list_folders())
}

#[tauri::command]
pub async fn create_folder(
    state: State<'_, AppState>,
    parent: Option<FolderId>,
    name: String,
) -> CommandResult<Folder> {
    state.with(|s| s.store.create_folder(parent, &name))
}

#[tauri::command]
pub async fn rename_folder(
    state: State<'_, AppState>,
    id: FolderId,
    name: String,
) -> CommandResult<()> {
    state.with(|s| s.store.rename_folder(id, &name))
}

#[tauri::command]
pub async fn move_folder(
    state: State<'_, AppState>,
    id: FolderId,
    parent: Option<FolderId>,
) -> CommandResult<FolderId> {
    state.with(|s| s.store.move_folder(id, parent))
}

#[tauri::command]
pub async fn trash_folder(state: State<'_, AppState>, id: FolderId) -> CommandResult<()> {
    state.with(|s| s.store.trash_folder(id))
}

#[tauri::command]
pub async fn restore_folder(state: State<'_, AppState>, id: FolderId) -> CommandResult<()> {
    state.with(|s| s.store.restore_folder(id))
}

#[tauri::command]
pub async fn list_tags(state: State<'_, AppState>) -> CommandResult<Vec<Tag>> {
    state.with(|s| s.store.list_tags())
}

#[tauri::command]
pub async fn list_versions(
    state: State<'_, AppState>,
    id: NoteId,
) -> CommandResult<Vec<NoteVersion>> {
    state.with(|s| s.store.list_versions(id))
}

#[tauri::command]
pub async fn restore_version(state: State<'_, AppState>, id: VersionId) -> CommandResult<Note> {
    state.with(|s| s.store.restore_version(id))
}

#[tauri::command]
pub async fn note_conflict(
    state: State<'_, AppState>,
    id: NoteId,
) -> CommandResult<Option<app_core::store::NoteConflict>> {
    state.with(|s| s.store.note_conflict(id))
}

#[tauri::command]
pub async fn conflicted_notes(state: State<'_, AppState>) -> CommandResult<Vec<NoteId>> {
    state.with(|s| s.store.conflicted_notes())
}

#[tauri::command]
pub async fn resolve_conflict(
    state: State<'_, AppState>,
    id: NoteId,
    keep: app_core::store::Resolution,
) -> CommandResult<()> {
    state.with(|s| s.store.resolve_conflict(id, &keep))
}
