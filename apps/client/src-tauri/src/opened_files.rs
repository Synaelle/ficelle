// SPDX-License-Identifier: AGPL-3.0-or-later

//! The file viewer: a Markdown or text file opened with the app, in a
//! window of its own on desktops (`#file`), over the app's screen on the
//! phone. It reads and writes only the file it was opened for: the page
//! never names a path, the window's label does. It needs no unlocked vault,
//! and nothing of the file reaches the vault unless the person imports it,
//! which asks for the unlock.
//!
//! The viewer is kept apart so it can be taken out whole. Everything it
//! adds to the rest of the app is marked "file viewer" (search for it),
//! except in the files that take no comment. Taking it out:
//! - this file, and the marked lines of `lib.rs` (module, state, commands,
//!   the launch with files, and `.run`, which then takes `|_, _| {}`);
//! - the marked line of `windows::launched_again`, so a second launch
//!   always shows the main window;
//! - the page: `src/fileViewer/`, the marked lines of `main.ts`, `App.svelte`
//!   and `lib/windows.ts`, `takeFiles` in `lib/inbox.ts`, and `[file]` in
//!   `packages/locales/en.toml`;
//! - Android: the marked intent filter of `AndroidManifest.xml`, the marked
//!   lines of `Inbox.kt` and `MainActivity.kt`;
//! - the system's side, unmarked: `fileAssociations` in `tauri.conf.json`
//!   (the build then empties the generated block of `AndroidManifest.xml`),
//!   `%F` and `MimeType` in the three `linux/*.desktop` files and in
//!   `tools/brand.mjs`, `file-*` in `capabilities/`.
//!
//! Putting it back is the same list the other way.

use std::collections::HashMap;
use std::path::Path;
#[cfg(desktop)]
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime, State, WebviewWindow};

use crate::error::{CommandError, CommandResult};
#[cfg(desktop)]
use crate::windows::Windows;

#[derive(Default)]
pub struct OpenedFiles(Mutex<Inner>);

#[derive(Default)]
struct Inner {
    /// The file each file window edits, by label: a path, or a `content://`
    /// address on Android.
    files: HashMap<String, String>,
    /// A note imported from a file window, for the main window to open.
    to_open: Option<String>,
}

impl OpenedFiles {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A window closed: it holds no file any more.
    pub fn forget(&self, label: &str) {
        self.lock().files.remove(label);
    }
}

/// Opens the files a launch names. False when it names none, and the
/// launch means the app itself.
#[cfg(desktop)]
pub fn open_from_args<R: Runtime>(
    app: &AppHandle<R>,
    args: Vec<String>,
    cwd: &Path,
) -> tauri::Result<bool> {
    let files = files_in(args.into_iter().skip(1), cwd);
    if files.is_empty() {
        return Ok(false);
    }
    open_files(app, &files)?;
    Ok(true)
}

/// The extensions the app opens, and whether each is plain text.
const OPENS: [(&str, bool); 3] = [("md", false), ("markdown", false), ("txt", true)];

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
#[cfg(desktop)]
pub fn files_in(args: impl IntoIterator<Item = String>, cwd: &Path) -> Vec<PathBuf> {
    args.into_iter()
        .filter(|arg| !arg.starts_with('-') && plain_text(arg).is_some())
        .map(|arg| cwd.join(arg))
        .filter_map(|path| path.canonicalize().ok())
        .filter(|path| path.is_file())
        .collect()
}

/// Opens a window for each file, or brings forward the one it already has.
#[cfg(desktop)]
pub fn open_files<R: Runtime>(app: &AppHandle<R>, paths: &[PathBuf]) -> tauri::Result<()> {
    let windows = app.state::<Windows>();
    let opened = app.state::<OpenedFiles>();
    for path in paths {
        let text = path.to_string_lossy().into_owned();
        let open = opened
            .lock()
            .files
            .iter()
            .find(|(_, file)| **file == text)
            .map(|(label, _)| label.clone());
        if let Some(window) = open.and_then(|label| app.get_webview_window(&label)) {
            crate::windows::bring(&window);
            continue;
        }
        let label = windows.label("file");
        let name = path
            .file_name()
            .map_or_else(|| text.clone(), |n| n.to_string_lossy().into_owned());
        opened.lock().files.insert(label.clone(), text);
        if let Err(err) = crate::windows::window(app, &label, "index.html#file", &name).build() {
            opened.forget(&label);
            return Err(err);
        }
    }
    Ok(())
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

fn file_of(opened: &OpenedFiles, label: &str) -> CommandResult<String> {
    opened
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

/// macOS hands the files opened with the app as an event, not as arguments.
#[cfg(target_os = "macos")]
pub fn on_run_event(app: &AppHandle, event: tauri::RunEvent) {
    if let tauri::RunEvent::Opened { urls } = event {
        let files: Vec<_> = urls
            .iter()
            .filter_map(|url| url.to_file_path().ok())
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        crate::windows::launched_again(
            app,
            [String::new()].into_iter().chain(files).collect(),
            "/",
        );
    }
}

#[cfg(not(target_os = "macos"))]
pub fn on_run_event(_: &AppHandle, _: tauri::RunEvent) {}

/// Reads the calling window's file.
#[tauri::command]
pub async fn opened_file_read<R: Runtime>(
    app: AppHandle<R>,
    window: WebviewWindow<R>,
    opened: State<'_, OpenedFiles>,
) -> CommandResult<OpenedFile> {
    let file = file_of(&opened, window.label())?;
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
    opened: State<'_, OpenedFiles>,
    text: String,
    stamp: Option<String>,
) -> CommandResult<Option<String>> {
    let file = file_of(&opened, window.label())?;
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
    opened: State<'_, OpenedFiles>,
    address: String,
) -> CommandResult<()> {
    if !crate::picked::is_document(&address) {
        return Err(CommandError::new("invalid", "not a document address"));
    }
    opened
        .lock()
        .files
        .insert(window.label().to_owned(), address);
    Ok(())
}

/// Brings the main window forward on a note imported from a file: it takes
/// the note with [`main_window_take_note`] once ready, when it was closed,
/// or on the `note-to-open` event.
#[cfg(desktop)]
#[tauri::command]
pub async fn main_window_show<R: Runtime>(
    app: AppHandle<R>,
    opened: State<'_, OpenedFiles>,
    id: String,
) -> CommandResult<()> {
    opened.lock().to_open = Some(id);
    crate::windows::show_main(&app).map_err(|e| CommandError::new("unknown", e.to_string()))?;
    let _ = tauri::Emitter::emit_to(&app, "main", "note-to-open", ());
    Ok(())
}

/// The phone's viewer covers the app's own screen, which stays as it was.
#[cfg(mobile)]
#[tauri::command]
pub async fn main_window_show(opened: State<'_, OpenedFiles>, id: String) -> CommandResult<()> {
    opened.lock().to_open = Some(id);
    Ok(())
}

/// The note an imported file became, once, for the main window to open.
#[tauri::command]
pub async fn main_window_take_note(
    opened: State<'_, OpenedFiles>,
) -> CommandResult<Option<String>> {
    Ok(opened.lock().to_open.take())
}

/// Whether the main window is open, whose auto-lock covers the notes.
#[tauri::command]
pub async fn main_window_open<R: Runtime>(app: AppHandle<R>) -> CommandResult<bool> {
    Ok(app.get_webview_window("main").is_some())
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
}
