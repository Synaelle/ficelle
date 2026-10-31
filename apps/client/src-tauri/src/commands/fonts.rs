// SPDX-License-Identifier: AGPL-3.0-or-later

//! Fonts for notes and code, over the theme's. A font from the app's list is
//! downloaded once, when chosen, and kept beside the vault in `fonts/<id>/`:
//! offline afterwards. It comes from the project's own host, which mirrors
//! Fontsource's open font files (`FONTS_URL`), so no third party sees which
//! font a device loads; from Fontsource on jsDelivr only when that fails.
//! The Carbon and Classic themes' fonts and OpenDyslexic come the same way
//! when first needed, the app does not carry them (`THEME_FONTS` in
//! lib/fonts.ts). Only those two addresses are ever asked, with a font id and
//! file names checked here, so the page cannot make the app fetch anything
//! else.
//! Fonts installed on the device are listed too, they need no download.

use std::io::Read;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use serde::Serialize;
use tauri::State;
use tauri::ipc::Response;

use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

const CDN: &str = "https://cdn.jsdelivr.net/fontsource/fonts";
/// A font file is tens of kilobytes, this is far above any.
const MAX_FILE: u64 = 4 * 1024 * 1024;
/// The files asked for each font: latin and its extension, regular, bold and
/// their italics. Only the first is needed, a font without italics is fine.
pub const FILES: [&str; 8] = [
    "latin-400-normal",
    "latin-700-normal",
    "latin-400-italic",
    "latin-700-italic",
    "latin-ext-400-normal",
    "latin-ext-700-normal",
    "latin-ext-400-italic",
    "latin-ext-700-italic",
];

/// Every file the page may ask for: [`FILES`], the medium weight, and the
/// variable files (`wght`) of a variable font, whose id ends in `:vf`.
const KNOWN: [&str; 14] = [
    "latin-400-normal",
    "latin-700-normal",
    "latin-400-italic",
    "latin-700-italic",
    "latin-ext-400-normal",
    "latin-ext-700-normal",
    "latin-ext-400-italic",
    "latin-ext-700-italic",
    "latin-500-normal",
    "latin-ext-500-normal",
    "latin-wght-normal",
    "latin-wght-italic",
    "latin-ext-wght-normal",
    "latin-ext-wght-italic",
];

fn invalid(what: &str) -> CommandError {
    CommandError::new("invalid", format!("not a font {what}"))
}

/// A Fontsource id: lowercase letters, digits and dashes, `:vf` after it
/// for a variable font.
fn checked_id(id: &str) -> CommandResult<&str> {
    let name = id.strip_suffix(":vf").unwrap_or(id);
    let ok = !name.is_empty()
        && id.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if ok { Ok(id) } else { Err(invalid("id")) }
}

fn checked_file(name: &str) -> CommandResult<&str> {
    KNOWN
        .contains(&name)
        .then_some(name)
        .ok_or_else(|| invalid("file"))
}

/// The files asked for, [`FILES`] when the page names none. The first is the
/// one a font cannot do without.
fn checked_files(files: Option<Vec<String>>) -> CommandResult<Vec<String>> {
    let files = files.unwrap_or_else(|| FILES.map(str::to_owned).to_vec());
    if files.is_empty() || files.len() > KNOWN.len() {
        return Err(invalid("file list"));
    }
    for name in &files {
        checked_file(name)?;
    }
    Ok(files)
}

/// `fonts/<id>`, `fonts/<id>@vf` for a variable font: a folder name has no `:`.
fn folder(state: &AppState, id: &str) -> CommandResult<PathBuf> {
    Ok(state
        .dir
        .join("fonts")
        .join(checked_id(id)?.replace(":vf", "@vf")))
}

fn failed(err: impl std::fmt::Display) -> CommandError {
    CommandError::new("offline", err.to_string())
}

/// One file: `None` when the font has no such file. A failure on the way is
/// tried once more, the files are small and the connection may hiccup.
fn fetch(agent: &ureq::Agent, url: &str) -> CommandResult<Option<Vec<u8>>> {
    let mut last = None;
    for _ in 0..3 {
        match agent.get(url).call() {
            Ok(response) => {
                let mut bytes = Vec::new();
                match response
                    .into_body()
                    .into_reader()
                    .take(MAX_FILE + 1)
                    .read_to_end(&mut bytes)
                {
                    Ok(_) => return Ok(Some(bytes)),
                    Err(err) => last = Some(err.to_string()),
                }
            }
            Err(ureq::Error::StatusCode(404)) => return Ok(None),
            Err(err) => last = Some(err.to_string()),
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    Err(failed(last.unwrap_or_default()))
}

/// Downloads a font's files, those it does not have yet. Returns the files
/// it has afterwards; fails when even the first one could not be had.
#[tauri::command]
pub async fn font_download(
    state: State<'_, AppState>,
    id: String,
    files: Option<Vec<String>>,
) -> CommandResult<Vec<String>> {
    let dir = folder(&state, &id)?;
    let files = checked_files(files)?;
    tauri::async_runtime::spawn_blocking(move || {
        std::fs::create_dir_all(&dir).map_err(|e| CommandError::new("file", e.to_string()))?;
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            // the name alone: an outside host has no need of the version
            .user_agent(app_brand::SLUG)
            .build()
            .into();
        let mut have = Vec::new();
        for name in &files {
            let path = dir.join(format!("{name}.woff2"));
            if path.is_file() {
                have.push(name.clone());
                continue;
            }
            let mirrored = format!(
                "{}/{}/{name}.woff2",
                app_brand::FONTS_URL.trim_end_matches('/'),
                id.replace(":vf", "@vf")
            );
            let fetched = match fetch(&agent, &mirrored) {
                Ok(Some(bytes)) if bytes.starts_with(b"wOF2") => Ok(Some(bytes)),
                // not mirrored, or the host does not answer: Fontsource's own
                _ => fetch(&agent, &format!("{CDN}/{id}@latest/{name}.woff2")),
            };
            let bytes = match fetched {
                Ok(Some(bytes)) => bytes,
                // the first file is needed, the others can wait
                Err(err) if *name == files[0] => return Err(err),
                // a font without italics or the extended latin, or a file
                // that did not come this time
                Ok(None) | Err(_) => continue,
            };
            // a woff2 file starts with wOF2, anything else is not kept
            if bytes.len() as u64 > MAX_FILE || !bytes.starts_with(b"wOF2") {
                continue;
            }
            let part = dir.join(format!("{name}.part"));
            std::fs::write(&part, &bytes).map_err(|e| CommandError::new("file", e.to_string()))?;
            std::fs::rename(&part, &path).map_err(|e| CommandError::new("file", e.to_string()))?;
            have.push(name.clone());
        }
        if have.contains(&files[0]) {
            Ok(have)
        } else {
            Err(failed("the font could not be downloaded"))
        }
    })
    .await
    .map_err(|e| CommandError::new("unknown", e.to_string()))?
}

/// The files of a font this device has, of those asked ([`FILES`] when none
/// are named), empty when it was never downloaded.
#[tauri::command]
pub async fn font_files(
    state: State<'_, AppState>,
    id: String,
    files: Option<Vec<String>>,
) -> CommandResult<Vec<String>> {
    let dir = folder(&state, &id)?;
    Ok(checked_files(files)?
        .into_iter()
        .filter(|name| dir.join(format!("{name}.woff2")).is_file())
        .collect())
}

/// One downloaded font file, as bytes for the page's `FontFace`.
#[tauri::command]
pub async fn font_file(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> CommandResult<Response> {
    let path = folder(&state, &id)?.join(format!("{}.woff2", checked_file(&name)?));
    let bytes = std::fs::read(path).map_err(|e| CommandError::new("notFound", e.to_string()))?;
    Ok(Response::new(bytes))
}

/// A font family installed on the device.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceFont {
    family: String,
    /// Every character the same width: fit for code.
    monospaced: bool,
}

/// Font families installed on this device, by name. Read once, the list
/// takes a moment to build.
#[tauri::command]
pub async fn system_fonts() -> CommandResult<Vec<DeviceFont>> {
    static FAMILIES: OnceLock<Vec<DeviceFont>> = OnceLock::new();
    if let Some(families) = FAMILIES.get() {
        return Ok(families.clone());
    }
    let families = tauri::async_runtime::spawn_blocking(installed_families)
        .await
        .map_err(|e| CommandError::new("unknown", e.to_string()))?;
    Ok(FAMILIES.get_or_init(|| families).clone())
}

fn installed_families() -> Vec<DeviceFont> {
    let mut db = fontdb::Database::new();
    #[cfg(target_os = "android")]
    db.load_fonts_dir("/system/fonts");
    #[cfg(not(target_os = "android"))]
    db.load_system_fonts();
    let mut families: Vec<DeviceFont> = db
        .faces()
        .filter_map(|face| {
            face.families.first().map(|(name, _)| DeviceFont {
                family: name.clone(),
                monospaced: face.monospaced,
            })
        })
        // Windows lists vertical writing fonts as "@Name", hidden fonts start with a dot
        .filter(|font| !font.family.starts_with('@') && !font.family.starts_with('.'))
        .collect();
    families.sort_by_key(|font| font.family.to_lowercase());
    families.dedup_by(|a, b| a.family == b.family);
    families
}

#[cfg(test)]
mod tests {
    use super::{FILES, KNOWN, checked_file, checked_files, checked_id};

    #[test]
    fn only_plain_ids_and_known_files() {
        assert!(checked_id("jetbrains-mono").is_ok());
        assert!(checked_id("source-serif-4").is_ok());
        assert!(checked_id("literata:vf").is_ok());
        let long = "a".repeat(65);
        for bad in [
            "", "../x", "a/b", "A", "x y", "a.b", ":vf", "a:vf:vf", "a:x", &long,
        ] {
            assert!(checked_id(bad).is_err(), "{bad}");
        }
        assert!(checked_file("latin-400-normal").is_ok());
        assert!(checked_file("../latin-400-normal").is_err());
        assert!(checked_file("latin-900-normal").is_err());
        assert!(checked_file("latin-ext-wght-italic").is_ok());
    }

    #[test]
    fn file_lists_default_to_the_usual_files_and_stay_known() {
        assert_eq!(checked_files(None).unwrap(), FILES.to_vec());
        let wght = vec!["latin-wght-normal".to_owned()];
        assert_eq!(checked_files(Some(wght.clone())).unwrap(), wght);
        assert!(checked_files(Some(vec![])).is_err());
        assert!(checked_files(Some(vec!["latin-400-normal".into(), "x".into()])).is_err());
        let too_many = vec!["latin-400-normal".to_owned(); KNOWN.len() + 1];
        assert!(checked_files(Some(too_many)).is_err());
    }
}
