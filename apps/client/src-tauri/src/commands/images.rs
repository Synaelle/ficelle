// SPDX-License-Identifier: AGPL-3.0-or-later

//! Images pasted and files dropped into a note, read here rather than from
//! the page's events. `WebKitGTK` gives the page only a path when a file was
//! copied or dragged from a file manager, and the page may not read paths
//! (`picked.rs`). Here the system's clipboard, or the window's drop, says which
//! files they were, and only images among the copied ones are read, so a
//! script in the page reaches nothing the person did not copy or drop.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use tauri::ipc::Response;
use tauri::{AppHandle, DragDropEvent, Manager, Runtime, State, Url, Window, WindowEvent};

use crate::error::{CommandError, CommandResult};

/// A file a note can take, by name and type, in the order pasted or dropped.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteFile {
    name: String,
    mime: String,
}

/// Image files, as in the picker's image filter; others go in as a path.
const IMAGES: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("avif", "image/avif"),
    ("bmp", "image/bmp"),
];
/// Copied file lists are short; a whole folder is not taken as images.
#[cfg_attr(
    mobile,
    expect(dead_code, reason = "a phone has no copied file lists or drops")
)]
const MOST_FILES: usize = 20;
/// The largest image read from a file, a guard against a huge file.
#[cfg_attr(
    mobile,
    expect(dead_code, reason = "a phone has no copied file lists or drops")
)]
const LARGEST: u64 = 64 * 1024 * 1024;

fn mime_of(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    IMAGES.iter().find(|(e, _)| *e == ext).map(|(_, m)| *m)
}

/// An image file a note can take: an image by its name, not too large.
#[cfg_attr(
    mobile,
    expect(dead_code, reason = "a phone has no copied file lists or drops")
)]
fn usable(path: &Path) -> bool {
    mime_of(path).is_some()
        && path
            .metadata()
            .is_ok_and(|m| m.is_file() && m.len() <= LARGEST)
}

/// The image files a copied file list names: `file://` addresses (KDE, the
/// `text/uri-list` form) or absolute paths (GNOME), one per line.
#[cfg_attr(
    mobile,
    expect(dead_code, reason = "a phone has no copied file lists or drops")
)]
fn image_files(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            if line.starts_with("file://") {
                Url::parse(line).ok()?.to_file_path().ok()
            } else {
                let path = PathBuf::from(line);
                path.is_absolute().then_some(path)
            }
        })
        .filter(|path| usable(path))
        .take(MOST_FILES)
        .collect()
}

fn described(paths: &[PathBuf]) -> Vec<NoteFile> {
    paths
        .iter()
        .map(|path| NoteFile {
            name: path
                .file_name()
                .map_or_default(|n| n.to_string_lossy().into_owned()),
            mime: mime_of(path).unwrap_or("image/png").to_owned(),
        })
        .collect()
}

fn read(path: &Path) -> CommandResult<Response> {
    let bytes = std::fs::read(path).map_err(|e| CommandError::new("notFound", e.to_string()))?;
    Ok(Response::new(bytes))
}

#[cfg(desktop)]
fn copied_files<R: Runtime>(app: &AppHandle<R>) -> Vec<PathBuf> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    app.clipboard()
        .read_text()
        .map_or_default(|text| image_files(&text))
}

/// A copied picture (not a file), as PNG.
#[cfg(desktop)]
fn copied_picture<R: Runtime>(app: &AppHandle<R>) -> Option<Vec<u8>> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    let image = app.clipboard().read_image().ok()?;
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, image.width(), image.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .ok()?
        .write_image_data(image.rgba())
        .ok()?;
    Some(out)
}

/// The images on the clipboard: the copied image files, or a copied picture.
#[tauri::command]
pub async fn clipboard_images<R: Runtime>(app: AppHandle<R>) -> CommandResult<Vec<NoteFile>> {
    #[cfg(desktop)]
    {
        let files = copied_files(&app);
        if !files.is_empty() {
            return Ok(described(&files));
        }
        if copied_picture(&app).is_some() {
            return Ok(vec![NoteFile {
                name: "pasted.png".to_owned(),
                mime: "image/png".to_owned(),
            }]);
        }
    }
    let _ = app;
    Ok(Vec::new())
}

/// One of [`clipboard_images`], as bytes, read again from the clipboard.
#[tauri::command]
pub async fn clipboard_image<R: Runtime>(
    app: AppHandle<R>,
    index: usize,
) -> CommandResult<Response> {
    #[cfg(desktop)]
    {
        let files = copied_files(&app);
        if let Some(path) = files.get(index) {
            return read(path);
        }
        if files.is_empty()
            && index == 0
            && let Some(png) = copied_picture(&app)
        {
            return Ok(Response::new(png));
        }
    }
    let _ = (app, index);
    Err(CommandError::new("notFound", "the clipboard changed"))
}

/// The files of the last drop on a window, which the system named.
#[derive(Default)]
pub struct Dropped(Mutex<Vec<PathBuf>>);

/// Keeps the files of a drop, before the page hears of it: files only, a
/// folder is not taken. Drops reach here only where the window takes them
/// itself (Linux, see the window config).
#[cfg_attr(
    mobile,
    expect(dead_code, reason = "a phone has no copied file lists or drops")
)]
pub fn remember_drop<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    if let WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) = event
        && let Some(dropped) = window.try_state::<Dropped>()
        && let Ok(mut kept) = dropped.0.lock()
    {
        *kept = paths
            .iter()
            .filter(|p| p.is_file())
            .take(MOST_FILES)
            .cloned()
            .collect();
    }
}

/// The files of the last drop, with an image's type; other files have
/// none here, the note takes them as they are.
#[tauri::command]
pub async fn dropped_files(dropped: State<'_, Dropped>) -> CommandResult<Vec<NoteFile>> {
    let kept = dropped
        .0
        .lock()
        .map_err(|_| CommandError::new("unknown", "drop lock"))?;
    Ok(kept
        .iter()
        .map(|path| NoteFile {
            name: path
                .file_name()
                .map_or_default(|n| n.to_string_lossy().into_owned()),
            mime: mime_of(path).unwrap_or_default().to_owned(),
        })
        .collect())
}

/// One of [`dropped_files`], as bytes. A file larger than a note takes is
/// refused before it is read.
#[tauri::command]
pub async fn dropped_file(dropped: State<'_, Dropped>, index: usize) -> CommandResult<Response> {
    let path = dropped
        .0
        .lock()
        .map_err(|_| CommandError::new("unknown", "drop lock"))?
        .get(index)
        .cloned()
        .ok_or_else(|| CommandError::new("notFound", "no such dropped file"))?;
    let size = path
        .metadata()
        .map_err(|e| CommandError::new("notFound", e.to_string()))?
        .len();
    if size > super::files::MAX_ATTACHMENT_BYTES as u64 {
        return Err(CommandError::new("tooLarge", "this file is too large"));
    }
    read(&path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copied_lists_keep_only_existing_images() {
        let dir = std::env::temp_dir().join(format!("ficelle-clipboard-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cat = dir.join("a cat.png");
        let notes = dir.join("notes.txt");
        std::fs::write(&cat, b"png").unwrap();
        std::fs::write(&notes, b"text").unwrap();
        let url = Url::from_file_path(&cat).unwrap();
        let text = format!(
            "{url}\n{}\n# a comment\nrelative.png\n{}\n",
            notes.display(),
            dir.join("gone.jpg").display()
        );
        assert_eq!(image_files(&text), vec![cat.clone()]);
        assert_eq!(image_files(&cat.display().to_string()), vec![cat]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn mime_follows_the_extension() {
        assert_eq!(mime_of(Path::new("/a/B.JPG")), Some("image/jpeg"));
        assert_eq!(mime_of(Path::new("/a/b.svg")), None);
    }
}
