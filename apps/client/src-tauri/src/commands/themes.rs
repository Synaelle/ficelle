// SPDX-License-Identifier: AGPL-3.0-or-later

//! Imported themes, kept beside the vault (see `app_core::themes`), and the
//! small text files a theme is imported from or exported to.

use std::io::{Read, Write};

use app_core::themes::{MAX_THEME, Themes};
use tauri::State;

use crate::error::{CommandError, CommandResult};
use crate::picked::{is_document, open_document, picked_path};
use crate::state::AppState;

fn themes(state: &AppState) -> Themes {
    Themes::new(state.dir.join("themes"))
}

/// Every imported theme, as its json text.
#[tauri::command]
pub async fn themes_list(state: State<'_, AppState>) -> CommandResult<Vec<String>> {
    Ok(themes(&state).list()?)
}

/// Keeps a theme the page checked, returns its id.
#[tauri::command]
pub async fn theme_save(state: State<'_, AppState>, json: String) -> CommandResult<String> {
    Ok(themes(&state).save(&json)?)
}

#[tauri::command]
pub async fn theme_remove(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    Ok(themes(&state).remove(&id)?)
}

/// A picked text file (a theme), at most the size of a theme.
#[tauri::command]
pub async fn read_text_file(app: tauri::AppHandle, path: String) -> CommandResult<String> {
    let file = if is_document(&path) {
        open_document(&app, &path, false)?
    } else {
        std::fs::File::open(picked_path(&app, &path)?)
            .map_err(|e| CommandError::new("file", e.to_string()))?
    };
    let mut text = String::new();
    // one byte more than allowed tells a file that is too large
    file.take(MAX_THEME as u64 + 1)
        .read_to_string(&mut text)
        .map_err(|e| CommandError::new("file", e.to_string()))?;
    if text.len() > MAX_THEME {
        return Err(CommandError::new("tooLarge", "the file is too large"));
    }
    Ok(text)
}

/// Writes a text file where the person chose (an exported theme).
#[tauri::command]
pub async fn write_text_file(
    app: tauri::AppHandle,
    path: String,
    text: String,
) -> CommandResult<()> {
    let mut file = if is_document(&path) {
        open_document(&app, &path, true)?
    } else {
        std::fs::File::create(picked_path(&app, &path)?)
            .map_err(|e| CommandError::new("file", e.to_string()))?
    };
    file.write_all(text.as_bytes())
        .map_err(|e| CommandError::new("file", e.to_string()))
}
