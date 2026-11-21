// SPDX-License-Identifier: AGPL-3.0-or-later

//! Windows beside the main one: a Markdown or text file opened with the app
//! (`#file`), and a note in a window of its own (`#note=<id>`).
//!
//! A file window reads and writes only the file it was opened for: the page
//! never names a path, the window's label does. It needs no unlocked vault,
//! and nothing of the file reaches the vault unless the person imports it.
//!
//! A note is open in one place at a time. Every window says which note it
//! shows, and opening that note elsewhere brings its window forward instead.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime, State, WebviewWindow};

use crate::error::{CommandError, CommandResult};

/// The extensions the app opens, and whether each is plain text.
const OPENS: [(&str, bool); 3] = [("md", false), ("markdown", false), ("txt", true)];

#[derive(Default)]
pub struct Windows(Mutex<Inner>);

#[derive(Default)]
struct Inner {
    next: u32,
    /// The file each file window edits, by label: a path, or a `content://`
    /// address on Android.
    files: HashMap<String, String>,
    /// The note each window shows, by label.
    notes: HashMap<String, String>,
    /// A note imported from a file window, for the main window to open.
    to_open: Option<String>,
}

impl Windows {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A window closed: it holds no file and no note any more.
    pub fn forget(&self, label: &str) {
        let mut inner = self.lock();
        inner.files.remove(label);
        inner.notes.remove(label);
    }

    fn label(&self, kind: &str) -> String {
        let mut inner = self.lock();
        inner.next += 1;
        format!("{kind}-{}", inner.next)
    }
}

/// A file the app opens, by its extension; true when it is plain text.
fn plain_text(path: &str) -> Option<bool> {
    let extension = Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
    OPENS
        .iter()
        .find(|(known, _)| *known == extension)
        .map(|(_, plain)| *plain)
}

/// The files among a launch's arguments, as full paths. Options and other
/// files are left out.
pub fn files_in(args: impl IntoIterator<Item = String>, cwd: &Path) -> Vec<PathBuf> {
    args.into_iter()
        .filter(|arg| !arg.starts_with('-') && plain_text(arg).is_some())
        .map(|arg| cwd.join(arg))
        .filter_map(|path| path.canonicalize().ok())
        .filter(|path| path.is_file())
        .collect()
}

/// A window's builder with what every window of the app shares.
#[cfg(desktop)]
fn window<'a, R: Runtime>(
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
    if let Some(dir) = crate::portable::webview_dir(app) {
        builder = builder.data_directory(dir);
    }
    builder
}

#[cfg(desktop)]
fn bring(window: &WebviewWindow<impl Runtime>) {
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

/// Opens a window for each file, or brings forward the one it already has.
#[cfg(desktop)]
pub fn open_files<R: Runtime>(app: &AppHandle<R>, paths: &[PathBuf]) -> tauri::Result<()> {
    let windows = app.state::<Windows>();
    for path in paths {
        let text = path.to_string_lossy().into_owned();
        let open = windows
            .lock()
            .files
            .iter()
            .find(|(_, file)| **file == text)
            .map(|(label, _)| label.clone());
        if let Some(window) = open.and_then(|label| app.get_webview_window(&label)) {
            bring(&window);
            continue;
        }
        let label = windows.label("file");
        let name = path
            .file_name()
            .map_or_else(|| text.clone(), |n| n.to_string_lossy().into_owned());
        windows.lock().files.insert(label.clone(), text);
        if let Err(err) = window(app, &label, "index.html#file", &name).build() {
            windows.forget(&label);
            return Err(err);
        }
    }
    Ok(())
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

/// A second launch of the app: its files open here, without files it means
/// the app itself.
#[cfg(desktop)]
pub fn launched_again<R: Runtime>(app: &AppHandle<R>, args: Vec<String>, cwd: &str) {
    let files = files_in(args.into_iter().skip(1), Path::new(cwd));
    let done = if files.is_empty() {
        show_main(app)
    } else {
        open_files(app, &files)
    };
    if let Err(err) = done {
        eprintln!("could not open a window: {err}");
    }
}

/// The file a file window edits, as the page shows it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedFile {
    name: String,
    text: String,
    /// A `.txt` file, shown and saved as plain text.
    plain: bool,
    /// When the file was last changed, to find a change made by another app
    /// before writing over it. None where the system does not say. Text: the
    /// nanoseconds are beyond what a number in the page holds exactly.
    stamp: Option<String>,
    /// Made read-only by its owner, so never written.
    read_only: bool,
}

fn file_of(windows: &Windows, label: &str) -> CommandResult<String> {
    windows
        .lock()
        .files
        .get(label)
        .cloned()
        .ok_or_else(|| CommandError::new("notFound", "this window has no file"))
}

fn stamp(path: &Path) -> Option<String> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let since = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(since.as_nanos().to_string())
}

fn file_error(err: &std::io::Error) -> CommandError {
    CommandError::new("file", err.to_string())
}

/// A file in another encoding than UTF-8 is not refused like a missing one:
/// the page says why.
fn read_error(err: &std::io::Error) -> CommandError {
    if err.kind() == std::io::ErrorKind::InvalidData {
        CommandError::new("notText", err.to_string())
    } else {
        file_error(err)
    }
}

/// Reads the calling window's file.
#[tauri::command]
pub async fn opened_file_read<R: Runtime>(
    app: AppHandle<R>,
    window: WebviewWindow<R>,
    windows: State<'_, Windows>,
) -> CommandResult<OpenedFile> {
    let file = file_of(&windows, window.label())?;
    if crate::picked::is_document(&file) {
        use std::io::Read;
        let mut text = String::new();
        crate::picked::open_document(&app, &file, false)?
            .read_to_string(&mut text)
            .map_err(|e| read_error(&e))?;
        let name = crate::picked::document_name(&file);
        return Ok(OpenedFile {
            plain: plain_text(&name).unwrap_or(false),
            name,
            text,
            stamp: None,
            read_only: false,
        });
    }
    let path = Path::new(&file);
    let text = std::fs::read_to_string(path).map_err(|e| read_error(&e))?;
    Ok(OpenedFile {
        name: path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
        plain: plain_text(&file).unwrap_or(false),
        text,
        stamp: stamp(path),
        read_only: std::fs::metadata(path).is_ok_and(|meta| meta.permissions().readonly()),
    })
}

/// Writes the calling window's file and gives its new stamp. Fails with
/// `changedOnDisk` when another app changed the file since `stamp`, unless
/// `stamp` is None (write over it).
#[tauri::command]
pub async fn opened_file_write<R: Runtime>(
    app: AppHandle<R>,
    window: WebviewWindow<R>,
    windows: State<'_, Windows>,
    text: String,
    stamp: Option<String>,
) -> CommandResult<Option<String>> {
    let file = file_of(&windows, window.label())?;
    if crate::picked::is_document(&file) {
        use std::io::Write;
        crate::picked::open_document(&app, &file, true)?
            .write_all(text.as_bytes())
            .map_err(|e| file_error(&e))?;
        return Ok(None);
    }
    let path = Path::new(&file);
    let meta = std::fs::metadata(path);
    // moved or deleted by another app: written again only when asked
    if stamp.is_some() && meta.is_err() {
        return Err(CommandError::new(
            "fileGone",
            "the file was moved or deleted",
        ));
    }
    // a file someone made read-only stays as it is
    if meta.is_ok_and(|meta| meta.permissions().readonly()) {
        return Err(CommandError::new("readOnly", "the file is read-only"));
    }
    if stamp.is_some() && self::stamp(path) != stamp {
        return Err(CommandError::new(
            "changedOnDisk",
            "the file was changed by another app",
        ));
    }
    write_whole(path, &text).map_err(|e| file_error(&e))?;
    Ok(self::stamp(path))
}

/// Writes a file through a copy beside it, renamed over it once complete:
/// a crash or a full disk leaves the old file, never half of the new one.
/// A file with other names (hard links) or in a folder that takes no new
/// file is written in place, which keeps it one file.
fn write_whole(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    if linked(path) {
        return write_in_place(path, text);
    }
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("no file name"))?;
    let mut temporary = name.to_owned();
    temporary.push(".saving");
    let temporary = path.with_file_name(temporary);
    let written = (|| {
        let mut file = std::fs::File::create(&temporary)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        if let Ok(meta) = std::fs::metadata(path) {
            std::fs::set_permissions(&temporary, meta.permissions())?;
        }
        std::fs::rename(&temporary, path)
    })();
    match written {
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied && path.exists() => {
            let _ = std::fs::remove_file(&temporary);
            write_in_place(path, text)
        }
        Err(err) => {
            let _ = std::fs::remove_file(&temporary);
            Err(err)
        }
        Ok(()) => Ok(()),
    }
}

fn write_in_place(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()
}

#[cfg(unix)]
fn linked(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).is_ok_and(|meta| meta.nlink() > 1)
}

#[cfg(not(unix))]
fn linked(_: &Path) -> bool {
    false
}

/// Android: a file another app handed over to view ("open with"). It
/// becomes the main window's file, the phone's only window.
#[tauri::command]
pub async fn opened_file_take<R: Runtime>(
    window: WebviewWindow<R>,
    windows: State<'_, Windows>,
    address: String,
) -> CommandResult<()> {
    if !crate::picked::is_document(&address) {
        return Err(CommandError::new("invalid", "not a document address"));
    }
    windows
        .lock()
        .files
        .insert(window.label().to_owned(), address);
    Ok(())
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

/// Brings the main window forward on a note imported from a file: it takes
/// the note with [`main_window_take_note`] once ready, when it was closed,
/// or on the `note-to-open` event.
#[cfg(desktop)]
#[tauri::command]
pub async fn main_window_show<R: Runtime>(
    app: AppHandle<R>,
    windows: State<'_, Windows>,
    id: String,
) -> CommandResult<()> {
    windows.lock().to_open = Some(id);
    show_main(&app).map_err(|e| CommandError::new("unknown", e.to_string()))?;
    let _ = tauri::Emitter::emit_to(&app, "main", "note-to-open", ());
    Ok(())
}

/// The phone's viewer covers the app's own screen, which stays as it was.
#[cfg(mobile)]
#[tauri::command]
pub async fn main_window_show(windows: State<'_, Windows>, id: String) -> CommandResult<()> {
    windows.lock().to_open = Some(id);
    Ok(())
}

/// The note an imported file became, once, for the main window to open.
#[tauri::command]
pub async fn main_window_take_note(windows: State<'_, Windows>) -> CommandResult<Option<String>> {
    Ok(windows.lock().to_open.take())
}

/// Whether the main window is open, whose auto-lock covers the notes.
#[tauri::command]
pub async fn main_window_open<R: Runtime>(app: AppHandle<R>) -> CommandResult<bool> {
    Ok(app.get_webview_window("main").is_some())
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
    let _ = tauri::Emitter::emit(app, "vault-locked", ());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_markdown_and_text_only() {
        assert_eq!(plain_text("a/notes.md"), Some(false));
        assert_eq!(plain_text("README.MARKDOWN"), Some(false));
        assert_eq!(plain_text("todo.txt"), Some(true));
        assert_eq!(plain_text("photo.png"), None);
        assert_eq!(plain_text("md"), None);
    }

    #[test]
    fn launch_files_are_the_existing_ones() {
        let dir = std::env::temp_dir().join(format!("opened-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.md"), "# a").unwrap();
        std::fs::write(dir.join("b.png"), "").unwrap();
        let args = ["a.md", "b.png", "missing.md", "--flag.md"].map(String::from);
        let found = files_in(args, &dir);
        assert_eq!(found, vec![dir.join("a.md").canonicalize().unwrap()]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_write_replaces_the_whole_file() {
        let dir = std::env::temp_dir().join(format!("opened-write-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("n.md");
        std::fs::write(&path, "old text that is longer").unwrap();
        write_whole(&path, "new").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_hard_linked_file_stays_one_file() {
        let dir = std::env::temp_dir().join(format!("opened-link-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.md");
        std::fs::write(&path, "old").unwrap();
        std::fs::hard_link(&path, dir.join("b.md")).unwrap();
        write_whole(&path, "new").unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("b.md")).unwrap(), "new");
        std::fs::remove_dir_all(dir).unwrap();
    }

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
