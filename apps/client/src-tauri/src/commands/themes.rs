// SPDX-License-Identifier: AGPL-3.0-or-later

//! Imported themes, kept beside the vault (see `app_core::themes`).

use app_core::themes::Themes;
use tauri::State;

use crate::error::CommandResult;
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
