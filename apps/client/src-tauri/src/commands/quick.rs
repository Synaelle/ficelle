// SPDX-License-Identifier: AGPL-3.0-or-later

//! The desktop quick note window (lib/quickNote.ts), made here rather than
//! from the page: only a window made in code can keep the webview's files
//! next to the portable exe, as the main one does. Phones have no such
//! window.

use tauri::AppHandle;

use crate::error::{CommandError, CommandResult};

/// Opens the quick note window, or brings it back when it is already open,
/// in front and ready to type. A new window starts hidden and shows once its
/// page is drawn ([`quick_window_ready`]), not as an empty frame first.
/// Async: a window made in a synchronous command blocks on Windows.
#[cfg(desktop)]
#[tauri::command]
pub async fn quick_window_open(app: AppHandle, title: String) -> CommandResult<()> {
    use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

    let failed = |e: tauri::Error| CommandError::new("unknown", e.to_string());
    if let Some(open) = app.get_webview_window("quick") {
        open.unminimize().map_err(failed)?;
        return show(&open, &title);
    }
    {
        let mut builder =
            WebviewWindowBuilder::new(&app, "quick", WebviewUrl::App("index.html#quick".into()))
                .title(&title)
                .inner_size(460.0, 320.0)
                .min_inner_size(320.0, 220.0)
                .center()
                .always_on_top(true)
                .visible(false);
        if let Some(dir) = crate::portable::webview_dir(&app) {
            builder = builder.data_directory(dir);
        }
        builder.build().map_err(failed)?;
    }
    Ok(())
}

/// The quick note page is drawn: the window shows, in front.
#[cfg(desktop)]
#[tauri::command]
pub async fn quick_window_ready(app: AppHandle, title: String) -> CommandResult<()> {
    use tauri::Manager;

    match app.get_webview_window("quick") {
        Some(window) => show(&window, &title),
        None => Ok(()),
    }
}

#[cfg(desktop)]
fn show(window: &tauri::WebviewWindow, title: &str) -> CommandResult<()> {
    window
        .show()
        .map_err(|e| CommandError::new("unknown", e.to_string()))?;
    let _ = window.set_focus();
    #[cfg(windows)]
    forward::bring(title);
    #[cfg(not(windows))]
    let _ = title;
    Ok(())
}

/// Windows keeps the foreground for the app the person is in: a window
/// opened from a shortcut pressed elsewhere would show without the keys.
/// Sharing that app's input for a moment lets ours take it over.
#[cfg(windows)]
mod forward {
    use winsafe::{AttachThreadInput, HWND};

    pub fn bring(title: &str) {
        // ours, by its title and this process, without its raw handle
        let Ok(Some(ours)) = HWND::FindWindow(None, Some(title)) else {
            return;
        };
        let (our_thread, our_process) = ours.GetWindowThreadProcessId();
        if our_process != std::process::id() {
            return;
        }
        let Some(front) = HWND::GetForegroundWindow() else {
            let _ = ours.SetForegroundWindow();
            return;
        };
        let (front_thread, _) = front.GetWindowThreadProcessId();
        let shared =
            front_thread != our_thread && AttachThreadInput(front_thread, our_thread, true).is_ok();
        let _ = ours.BringWindowToTop();
        let _ = ours.SetForegroundWindow();
        if shared {
            let _ = AttachThreadInput(front_thread, our_thread, false);
        }
    }
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

/// Phones have no quick note window.
#[cfg(mobile)]
#[tauri::command]
pub async fn quick_window_ready(app: AppHandle, title: String) -> CommandResult<()> {
    drop((app, title));
    Ok(())
}
