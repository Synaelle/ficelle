// SPDX-License-Identifier: AGPL-3.0-or-later

//! Temporary issue reports for dev and preview builds, saved as markdown
//! folders the developer reads directly. Release builds refuse them. Goes
//! away when the real feedback feature exists.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use tauri::AppHandle;
use tauri::ipc::Request;

use crate::error::{CommandError, CommandResult};

/// Set by build.rs for every build that is not an official release.
const ENABLED: bool = cfg!(dev_reports);

/// Pasted screenshots are rarely above a few MB, this only stops accidents.
const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;

fn disabled() -> CommandError {
    CommandError::new(
        "unsupported",
        "reports only exist in dev and preview builds",
    )
}

/// The folder picked in the test tools settings when there is one, else the
/// one given at build time in `APP_REPORTS_DIR` when it is there, otherwise
/// the app data folder.
fn reports_root(app: &AppHandle, picked: Option<&str>) -> CommandResult<PathBuf> {
    if let Some(picked) = picked.filter(|p| !p.is_empty()) {
        let dir = PathBuf::from(picked);
        if !dir.is_absolute() || !dir.is_dir() {
            return Err(CommandError::new("notFound", "the reports folder is gone"));
        }
        return Ok(dir);
    }
    if let Some(dir) = option_env!("APP_REPORTS_DIR").map(Path::new)
        && dir.is_dir()
    {
        return Ok(dir.to_path_buf());
    }
    let data =
        crate::portable::data_dir(app).map_err(|e| CommandError::new("file", e.to_string()))?;
    Ok(data.join("reports"))
}

/// Folder names come from the ui, so they are limited to a safe alphabet.
fn safe_name(name: &str) -> CommandResult<&str> {
    let ok = !name.is_empty()
        && name.len() <= 80
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
        && !name.starts_with('.');
    ok.then_some(name)
        .ok_or_else(|| CommandError::new("invalid", "bad report or image name"))
}

fn io(err: &std::io::Error) -> CommandError {
    CommandError::new("file", err.to_string())
}

/// Where a report was saved: the folder name for adding images, the full
/// path to show.
#[derive(Serialize)]
pub struct SavedReport {
    name: String,
    path: String,
}

/// Where reports go now, for the settings to show.
#[tauri::command]
pub async fn dev_report_folder(app: AppHandle, folder: Option<String>) -> CommandResult<String> {
    if !ENABLED {
        return Err(disabled());
    }
    Ok(reports_root(&app, folder.as_deref())?
        .to_string_lossy()
        .into_owned())
}

/// Writes `report.md` into a new folder.
#[tauri::command]
pub async fn dev_report_create(
    app: AppHandle,
    name: String,
    markdown: String,
    folder: Option<String>,
) -> CommandResult<SavedReport> {
    if !ENABLED {
        return Err(disabled());
    }
    let root = reports_root(&app, folder.as_deref())?;
    let base = safe_name(&name)?;
    let mut name = base.to_owned();
    let mut n = 2;
    while root.join(&name).exists() {
        name = format!("{base}-{n}");
        n += 1;
    }
    let dir = root.join(&name);
    fs::create_dir_all(&dir).map_err(|e| io(&e))?;
    fs::write(dir.join("report.md"), markdown).map_err(|e| io(&e))?;
    let path = dir.to_string_lossy().into_owned();
    Ok(SavedReport { name, path })
}

/// Saves one image next to a report. Raw body like attachments, with the
/// report folder name in `x-report`, the file name in `x-file-name` and the
/// picked reports folder, if any, in `x-folder` (percent-encoded).
#[tauri::command]
pub async fn dev_report_add_image(app: AppHandle, request: Request<'_>) -> CommandResult<()> {
    if !ENABLED {
        return Err(disabled());
    }
    let bytes = super::files::body_bytes(&request)?;
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(CommandError::new("tooLarge", "images are limited to 20 MB"));
    }
    let header = |name: &str| {
        request
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| CommandError::new("invalid", format!("missing {name} header")))
    };
    // both names are checked, so the file can only land in a report folder
    let picked = request
        .headers()
        .get("x-folder")
        .and_then(|v| v.to_str().ok())
        .map(percent_decode);
    let dir = reports_root(&app, picked.as_deref())?.join(safe_name(header("x-report")?)?);
    let file = safe_name(header("x-file-name")?)?;
    if !dir.is_dir() {
        return Err(CommandError::new("notFound", "no such report"));
    }
    fs::write(dir.join(file), bytes).map_err(|e| io(&e))
}

/// Header values are ASCII, so the page sends the folder through
/// encodeURIComponent.
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
