// SPDX-License-Identifier: AGPL-3.0-or-later

//! The portable Windows exe (`pnpm build:portable`): everything it keeps,
//! the vault, the page's storage and the webview's own files, sits in a
//! folder next to the exe, so it moves with it. Windows Hello and the PIN
//! stay with the computer they were set on, elsewhere the password opens
//! the vault. It does not update itself, it says when a newer one is out.

use std::path::PathBuf;

use tauri::{AppHandle, Manager, Runtime};

/// This build is the portable exe: `APP_PORTABLE` was set when it was built.
pub const PORTABLE: bool = cfg!(windows) && option_env!("APP_PORTABLE").is_some();

/// Where this build keeps its data: next to the exe when portable, the
/// system's app data folder otherwise. A portable exe in a folder it cannot
/// write to falls back to the system's, rather than not starting.
pub fn data_dir<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<PathBuf> {
    if let Some(dir) = beside_exe() {
        return Ok(dir);
    }
    app.path().app_data_dir()
}

/// The webview's folder, set on every window: only the portable exe has one,
/// tauri picks the system's otherwise.
pub fn webview_dir<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    PORTABLE.then(|| data_dir(app).ok().map(|dir| dir.join("webview")))?
}

fn beside_exe() -> Option<PathBuf> {
    if !PORTABLE {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe
        .parent()?
        .join(format!("{} data", app_brand::DISPLAY_NAME));
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// The main window, which the portable overlay leaves to the code: its
/// config is used as is, with the webview's folder next to the exe.
pub fn open_main<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let Some(webview) = webview_dir(app) else {
        return Ok(());
    };
    let Some(config) = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "main" && !w.create)
    else {
        return Ok(());
    };
    tauri::WebviewWindowBuilder::from_config(app, config)?
        .data_directory(webview)
        .build()?;
    Ok(())
}
