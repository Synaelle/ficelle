// SPDX-License-Identifier: AGPL-3.0-or-later

//! Backups of the whole vault, and the markdown export as a zip.

use app_core::backup::{Backup, Manifest};
use app_core::crypto::KdfParams;
use app_core::store::RestoreReport;
use serde::Serialize;
use tauri::State;

use super::files::ExportSummary;
use crate::error::CommandResult;
use crate::picked::{is_document, open_document, picked_path};
use crate::state::AppState;

/// The name offered for a new backup.
#[tauri::command]
pub fn backup_default_name() -> String {
    app_core::backup::default_name(app_core::clock::Clock::now(&app_core::clock::SystemClock))
        .to_string_lossy()
        .into_owned()
}

/// Writes a backup of the vault, sealed with `password` when there is one.
#[tauri::command]
pub async fn backup_write(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    path: String,
    password: Option<String>,
) -> CommandResult<Manifest> {
    let password = password.as_deref().filter(|p| !p.is_empty());
    if is_document(&path) {
        let file = open_document(&app, &path, true)?;
        return state.quietly(|s| {
            app_core::backup::write_backup_to(
                &s.store,
                &s.blobs,
                file,
                password,
                KdfParams::DEFAULT,
            )
        });
    }
    let path = picked_path(&app, &path)?;
    state.quietly(|s| {
        app_core::backup::write_backup(&s.store, &s.blobs, &path, password, KdfParams::DEFAULT)
    })
}

/// A picked backup opened: from a path, or an Android document.
fn open_backup(
    app: &tauri::AppHandle,
    path: &str,
    password: Option<&str>,
) -> CommandResult<Backup> {
    Ok(if is_document(path) {
        Backup::open_file(open_document(app, path, false)?, password)?
    } else {
        Backup::open(&picked_path(app, path)?, password)?
    })
}

/// Whether a backup needs a password to open.
#[tauri::command]
pub async fn backup_encrypted(app: tauri::AppHandle, path: String) -> CommandResult<bool> {
    if is_document(&path) {
        let mut file = open_document(&app, &path, false)?;
        return Ok(app_core::backup::is_encrypted_file(&mut file)?);
    }
    Ok(app_core::backup::is_encrypted(&picked_path(&app, &path)?)?)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inspected {
    manifest: Manifest,
    /// Notes of the backup already in this vault, left as they are.
    present: usize,
}

/// What a backup holds and how much of it is here already, writing nothing.
#[tauri::command]
pub async fn backup_inspect(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    path: String,
    password: Option<String>,
) -> CommandResult<Inspected> {
    let mut backup = open_backup(&app, &path, password.as_deref())?;
    let data = backup.data()?;
    let ids: Vec<_> = data.notes.iter().map(|n| n.id).collect();
    let present = state.quietly(|s| s.store.notes_present(&ids))?.len();
    Ok(Inspected {
        manifest: backup.manifest().clone(),
        present,
    })
}

/// Puts a backup back into the vault.
#[tauri::command]
pub async fn backup_restore(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    path: String,
    password: Option<String>,
) -> CommandResult<RestoreReport> {
    let mut backup = open_backup(&app, &path, password.as_deref())?;
    state.with(|s| backup.restore(&mut s.store, &s.blobs))
}

/// The markdown export in one zip file.
#[tauri::command]
pub async fn export_markdown_zip(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> CommandResult<ExportSummary> {
    if is_document(&path) {
        let file = open_document(&app, &path, true)?;
        return state.with(|s| {
            let report = app_core::export::export_markdown_zip_to(&s.store, file, &s.blobs)?;
            Ok(ExportSummary::from(report))
        });
    }
    let path = picked_path(&app, &path)?;
    state.with(|s| {
        let report = app_core::export::export_markdown_zip(&s.store, &path, &s.blobs)?;
        Ok(ExportSummary::from(report))
    })
}
