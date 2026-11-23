// SPDX-License-Identifier: AGPL-3.0-or-later

//! Windows beside the main one: a note in a window of its own
//! (`#note=<id>`), and the windows every other one is built like. The
//! windows of files opened with the app are in [`crate::opened_files`].
//!
//! A note is open in one place at a time. Every window says which note it
//! shows, and opening that note elsewhere brings its window forward instead.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use tauri::{AppHandle, Manager, Runtime, State, WebviewWindow};

use crate::error::{CommandError, CommandResult};

#[derive(Default)]
pub struct Windows(Mutex<Inner>);

#[derive(Default)]
struct Inner {
    /// Windows of their own are a desktop's.
    #[cfg(desktop)]
    next: u32,
    /// The note each window shows, by label.
    notes: HashMap<String, String>,
}

impl Windows {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A window closed: it shows no note any more.
    pub fn forget(&self, label: &str) {
        self.lock().notes.remove(label);
    }

    /// A new window's label, `<kind>-<n>`.
    #[cfg(desktop)]
    pub(crate) fn label(&self, kind: &str) -> String {
        let mut inner = self.lock();
        inner.next += 1;
        format!("{kind}-{}", inner.next)
    }
}

/// A window's builder with what every window of the app shares. Dropped
/// files go where the main window's config sends them: to the page on
/// Windows, to the window on Linux (see `commands::images`).
#[cfg(desktop)]
pub(crate) fn window<'a, R: Runtime>(
    app: &'a AppHandle<R>,
    label: &str,
    page: &str,
    title: &str,
) -> tauri::WebviewWindowBuilder<'a, R, AppHandle<R>> {
    let mut builder =
        tauri::WebviewWindowBuilder::new(app, label, tauri::WebviewUrl::App(page.into()))
            .title(title)
            .inner_size(860.0, 720.0)
            .min_inner_size(400.0, 480.0)
            .visible(false);
    let page_takes_drops = app
        .config()
        .app
        .windows
        .iter()
        .any(|w| w.label == "main" && !w.drag_drop_enabled);
    if page_takes_drops {
        builder = builder.disable_drag_drop_handler();
    }
    if let Some(dir) = crate::portable::webview_dir(app) {
        builder = builder.data_directory(dir);
    }
    builder
}

#[cfg(desktop)]
pub(crate) fn bring(window: &WebviewWindow<impl Runtime>) {
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

/// Brings the main window forward, made again when it was closed.
#[cfg(desktop)]
pub fn show_main<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    match app.get_webview_window("main") {
        Some(main) => {
            bring(&main);
            Ok(())
        }
        None => crate::portable::open_main(app),
    }
}

/// A second launch of the app: the files it names open here, without files
/// it means the app itself.
#[cfg(desktop)]
pub fn launched_again<R: Runtime>(app: &AppHandle<R>, args: Vec<String>, cwd: &str) {
    // file viewer hook: see opened_files
    let done = match crate::opened_files::open_from_args(app, args, std::path::Path::new(cwd)) {
        Ok(true) => Ok(()),
        Ok(false) => show_main(app),
        Err(err) => Err(err),
    };
    if let Err(err) = done {
        eprintln!("could not open a window: {err}");
    }
}

/// The page has drawn: its window shows.
#[tauri::command]
pub async fn window_ready<R: Runtime>(window: WebviewWindow<R>) -> CommandResult<()> {
    #[cfg(desktop)]
    bring(&window);
    #[cfg(mobile)]
    drop(window);
    Ok(())
}

/// The note the calling window shows now, or none.
#[tauri::command]
pub async fn window_note_set<R: Runtime>(
    window: WebviewWindow<R>,
    windows: State<'_, Windows>,
    id: Option<String>,
) -> CommandResult<()> {
    let mut inner = windows.lock();
    match id {
        Some(id) => inner.notes.insert(window.label().to_owned(), id),
        None => inner.notes.remove(window.label()),
    };
    Ok(())
}

/// The window other than the caller's that shows a note.
fn showing(windows: &Windows, id: &str, caller: &str) -> Option<String> {
    windows
        .lock()
        .notes
        .iter()
        .find(|(label, note)| *note == id && *label != caller)
        .map(|(label, _)| label.clone())
}

/// Brings forward the other window that shows a note. False when no other
/// window shows it, and the caller may open it.
#[tauri::command]
pub async fn note_window_focus<R: Runtime>(
    app: AppHandle<R>,
    window: WebviewWindow<R>,
    windows: State<'_, Windows>,
    id: String,
) -> CommandResult<bool> {
    let Some(other) = showing(&windows, &id, window.label()) else {
        return Ok(false);
    };
    let Some(other) = app.get_webview_window(&other) else {
        windows.forget(&other);
        return Ok(false);
    };
    #[cfg(desktop)]
    bring(&other);
    #[cfg(mobile)]
    drop(other);
    Ok(true)
}

/// Opens a note in a window of its own, or brings forward the window that
/// shows it. The caller leaves the note first.
#[cfg(desktop)]
#[tauri::command]
pub async fn note_window_open<R: Runtime>(
    app: AppHandle<R>,
    window: WebviewWindow<R>,
    windows: State<'_, Windows>,
    id: String,
    title: String,
) -> CommandResult<()> {
    let failed = |e: tauri::Error| CommandError::new("unknown", e.to_string());
    if note_window_focus(app.clone(), window, windows.clone(), id.clone()).await? {
        return Ok(());
    }
    // ids are uuids: nothing in one needs escaping in the address
    if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(CommandError::new("invalid", "not a note id"));
    }
    let label = windows.label("note");
    windows.lock().notes.insert(label.clone(), id.clone());
    let page = format!("index.html#note={id}");
    if let Err(err) = self::window(&app, &label, &page, &title).build() {
        windows.forget(&label);
        return Err(failed(err));
    }
    Ok(())
}

/// Phones show one note at a time.
#[cfg(mobile)]
#[tauri::command]
pub async fn note_window_open(id: String, title: String) -> CommandResult<()> {
    drop((id, title));
    Err(CommandError::new(
        "unsupported",
        "notes open in their own window on desktops only",
    ))
}

/// How many note windows are open, which a lock waits for.
#[tauri::command]
pub async fn note_window_count<R: Runtime>(app: AppHandle<R>) -> CommandResult<usize> {
    Ok(app
        .webview_windows()
        .keys()
        .filter(|label| label.starts_with("note-"))
        .count())
}

/// The main window closed: the note windows go with it, they are part of
/// the notes it opened and its lock. Each saves before it goes.
#[cfg(desktop)]
pub fn main_closed<R: Runtime>(app: &AppHandle<R>) {
    for (label, window) in app.webview_windows() {
        if label.starts_with("note-") {
            let _ = window.close();
        }
    }
    lock_when_alone(app, "main");
}

/// With the main window and every note window gone, the notes lock: file
/// windows may stay open, and the main window's auto-lock no longer runs.
/// `closing` may still be listed while it goes.
#[cfg(desktop)]
pub fn lock_when_alone<R: Runtime>(app: &AppHandle<R>, closing: &str) {
    let open = app.webview_windows();
    if open
        .keys()
        .any(|label| label != closing && (label == "main" || label.starts_with("note-")))
    {
        return;
    }
    app.state::<crate::state::AppState>().close();
    crate::sync::files::clean_opened_copies(app);
    let _ = tauri::Emitter::emit(app, "vault-locked", ());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_shows_in_one_window() {
        let windows = Windows::default();
        windows.lock().notes.insert("main".into(), "n1".into());
        assert_eq!(showing(&windows, "n1", "note-1").as_deref(), Some("main"));
        assert_eq!(showing(&windows, "n1", "main"), None);
        windows.forget("main");
        assert_eq!(showing(&windows, "n1", "note-1"), None);
    }
}
