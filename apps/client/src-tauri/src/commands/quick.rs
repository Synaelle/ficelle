// SPDX-License-Identifier: AGPL-3.0-or-later

//! The desktop quick note window (lib/quickNote.ts), made here rather than
//! from the page: only a window made in code can keep the webview's files
//! next to the portable exe, as the main one does. Phones have no such
//! window.

use tauri::AppHandle;

use crate::error::{CommandError, CommandResult};

/// Opens the quick note window. The page shows it again when it is already
/// open. Async: a window made in a synchronous command blocks on Windows.
#[cfg(desktop)]
#[tauri::command]
pub async fn quick_window_open(app: AppHandle, title: String) -> CommandResult<()> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};

    let mut window =
        WebviewWindowBuilder::new(&app, "quick", WebviewUrl::App("index.html#quick".into()))
            .title(title)
            .inner_size(460.0, 320.0)
            .min_inner_size(320.0, 220.0)
            .center()
            .always_on_top(true)
            .focused(true);
    if let Some(dir) = crate::portable::webview_dir(&app) {
        window = window.data_directory(dir);
    }
    window
        .build()
        .map(drop)
        .map_err(|e| CommandError::new("unknown", e.to_string()))
}

/// Phones have no quick note window.
#[cfg(mobile)]
#[tauri::command]
pub async fn quick_window_open(app: AppHandle, title: String) -> CommandResult<()> {
    drop((app, title));
    Err(CommandError::new(
        "unsupported",
        "the quick note window is for desktops",
    ))
}
