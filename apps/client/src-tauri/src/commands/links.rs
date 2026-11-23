// SPDX-License-Identifier: AGPL-3.0-or-later

//! Search and wiki links.

use app_core::id::NoteId;
use app_core::model::{NoteSummary, SearchHit};
use app_core::store::LinkSuggestion;
use tauri::State;

use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

#[tauri::command]
pub async fn search(
    state: State<'_, AppState>,
    query: String,
    limit: usize,
) -> CommandResult<Vec<SearchHit>> {
    state.with(|s| s.store.search(&query, limit))
}

#[tauri::command]
pub async fn backlinks(state: State<'_, AppState>, id: NoteId) -> CommandResult<Vec<NoteSummary>> {
    state.with(|s| s.store.backlinks(id))
}

#[tauri::command]
pub async fn resolve_link(
    state: State<'_, AppState>,
    target: String,
) -> CommandResult<Option<NoteId>> {
    state.with(|s| s.store.resolve_link(&target))
}

#[tauri::command]
pub async fn link_suggestions(
    state: State<'_, AppState>,
    input: String,
    limit: usize,
) -> CommandResult<Vec<LinkSuggestion>> {
    state.with(|s| s.store.link_suggestions(&input, limit))
}

#[tauri::command]
pub async fn note_headings(state: State<'_, AppState>, id: NoteId) -> CommandResult<Vec<String>> {
    state.with(|s| s.store.note_headings(id))
}

/// Opens a link of a note in the system's browser, or its mail app for a
/// `mailto:` one. Only those: a note never starts a program or opens a file.
#[tauri::command]
pub async fn link_open(url: String) -> CommandResult<()> {
    let scheme = url.split_once(':').map(|(s, _)| s.to_ascii_lowercase());
    if !matches!(scheme.as_deref(), Some("http" | "https" | "mailto")) {
        return Err(CommandError::new("invalid", "not a web or mail link"));
    }
    #[cfg(desktop)]
    return crate::sync::browser::open_in_browser(&url)
        .map_err(|e| CommandError::new("browser", e.to_string()));
    #[cfg(mobile)]
    Err(CommandError::new(
        "unsupported",
        "the phone opens links itself",
    ))
}
