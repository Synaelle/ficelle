// SPDX-License-Identifier: AGPL-3.0-or-later

//! Images pasted or dropped into a note, read here rather than from the
//! page's events. `WebKitGTK` gives the page only a path when an image file was
//! copied or dragged from a file manager, and the page may not read paths
//! (`picked.rs`). Here the system's clipboard, or the window's drop, says which
//! files they were, and only images among them are read, so a script in the
//! page reaches nothing the person did not copy or drop.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use tauri::ipc::Response;
use tauri::{AppHandle, DragDropEvent, Manager, Runtime, State, Url, Window, WindowEvent};

use crate::error::{CommandError, CommandResult};

/// An image a note can take, in the order pasted or dropped.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageFile {
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
const MOST_FILES: usize = 20;
/// The largest image read from a file, a guard against a huge file.
const LARGEST: u64 = 64 * 1024 * 1024;

fn mime_of(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    IMAGES.iter().find(|(e, _)| *e == ext).map(|(_, m)| *m)
}

/// An image file a note can take: an image by its name, not too large.
fn usable(path: &Path) -> bool {
    mime_of(path).is_some()
        && path
            .metadata()
            .is_ok_and(|m| m.is_file() && m.len() <= LARGEST)
}

/// The image files a copied file list names: `file://` addresses (KDE, the
/// `text/uri-list` form) or absolute paths (GNOME), one per line.
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

fn described(paths: &[PathBuf]) -> Vec<ImageFile> {
    paths
        .iter()
        .map(|path| ImageFile {
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
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
        .map(|text| image_files(&text))
        .unwrap_or_default()
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
pub async fn clipboard_images<R: Runtime>(app: AppHandle<R>) -> CommandResult<Vec<ImageFile>> {
    #[cfg(desktop)]
    {
        let files = copied_files(&app);
        if !files.is_empty() {
            return Ok(described(&files));
        }
        if copied_picture(&app).is_some() {
            return Ok(vec![ImageFile {
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

/// The image files of the last drop on a window, which the system named.
#[derive(Default)]
pub struct Dropped(Mutex<Vec<PathBuf>>);

/// Keeps the image files of a drop, before the page hears of it. Drops reach
/// here only where the window takes them itself (Linux, see the window config).
pub fn remember_drop<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    if let WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) = event
        && let Some(dropped) = window.try_state::<Dropped>()
        && let Ok(mut kept) = dropped.0.lock()
    {
        *kept = paths
            .iter()
            .filter(|p| usable(p))
            .take(MOST_FILES)
            .cloned()
            .collect();
    }
}

/// The image files of the last drop.
#[tauri::command]
pub async fn dropped_images(dropped: State<'_, Dropped>) -> CommandResult<Vec<ImageFile>> {
    let kept = dropped
        .0
        .lock()
        .map_err(|_| CommandError::new("unknown", "drop lock"))?;
    Ok(described(&kept))
}

/// One of [`dropped_images`], as bytes.
#[tauri::command]
pub async fn dropped_image(dropped: State<'_, Dropped>, index: usize) -> CommandResult<Response> {
    let path = dropped
        .0
        .lock()
        .map_err(|_| CommandError::new("unknown", "drop lock"))?
        .get(index)
        .cloned()
        .ok_or_else(|| CommandError::new("notFound", "no such dropped file"))?;
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
