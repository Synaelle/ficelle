// SPDX-License-Identifier: AGPL-3.0-or-later

//! The fonts the app does not carry (lib/fonts.ts): OpenDyslexic, and Ubuntu
//! and JetBrains Mono for those who pick them in the editor settings.
//! Each is downloaded once, when first needed, and kept beside the vault in
//! `fonts/<id>/`, offline afterwards. It comes from the project's own host, which mirrors Fontsource's
//! open font files (`FONTS_URL`); from Fontsource on jsDelivr only when that
//! fails. Only those two addresses are ever asked, with a font id and file names
//! checked here, so the page cannot make the app fetch anything else.

use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

use tauri::State;
use tauri::ipc::Response;

use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

/// The Fontsource release asked for, the one the project's host mirrors.
const FONTSOURCE: &str = "5.3.0";
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

/// The SHA-256 of each file of the release above, as both hosts serve them.
/// Anything else, from either, is refused: the web view's font engine reads
/// these files, so only the ones checked here may reach it.
const HASHES: [(&str, &str, &str); 20] = [
    (
        "opendyslexic",
        "latin-400-normal",
        "f007004af3cda5d8076e57c943f8cc8d00a0da25988b1ae1048683d60e7cac1a",
    ),
    (
        "opendyslexic",
        "latin-700-normal",
        "dd9fa9c7991113b0dddefe9506a30ad26b48e302b7a8fb91719a4726e8fde85b",
    ),
    (
        "opendyslexic",
        "latin-400-italic",
        "eb6a1bacf7e7c87a08116af4cd00a82064fbe61647b9a0d4d70a339890268f88",
    ),
    (
        "opendyslexic",
        "latin-700-italic",
        "a20d82c2a1a0a6ab74796529aca31bfbae93e3e58f5a7360cba854dd98a15214",
    ),
    (
        "ubuntu",
        "latin-400-normal",
        "dabae363ac0ae6c3b2a137a32f7132b42520a8af252f87aa6c2198f2a79cf91c",
    ),
    (
        "ubuntu",
        "latin-700-normal",
        "3658a0717e347c1db8d80dd565584b9dc56769fbf87909c50a4a7740206ec5f7",
    ),
    (
        "ubuntu",
        "latin-400-italic",
        "bc79d1b312559bbb2fec09c60895b411a60403586df8907fc022afaae3be7971",
    ),
    (
        "ubuntu",
        "latin-700-italic",
        "c43326719ec42ca99c5f6ee1c6107ebe59f636812c4a0e93a58a2e7051db4930",
    ),
    (
        "ubuntu",
        "latin-ext-400-normal",
        "7db064fa5c5d305d684608744d621aa724ec4f1c6b00bb647697f4d8d3e78df1",
    ),
    (
        "ubuntu",
        "latin-ext-700-normal",
        "30073583012b73a5c05c290e601325850ddbc7c7a1241561ec9264b8744c2a86",
    ),
    (
        "ubuntu",
        "latin-ext-400-italic",
        "b923867df5e5862a03c79e4d38ad76d5102da1a5a01d8ccadc75c05e0786e8bd",
    ),
    (
        "ubuntu",
        "latin-ext-700-italic",
        "794cd2c5767e39c9273da3a321c0f65a599d6c5a08d24a6c7c3798bea6de00d7",
    ),
    (
        "jetbrains-mono",
        "latin-400-normal",
        "14425ba9c695763c1547f48a206b7aa60350a33ae23de09f0407877f3fcd89eb",
    ),
    (
        "jetbrains-mono",
        "latin-700-normal",
        "d0d4e818808f2a0ba39b2b09d1989366f63494e295f003c7ef436697378507e8",
    ),
    (
        "jetbrains-mono",
        "latin-400-italic",
        "87ddac4a62229787528cf4ac3fa58137b62a9aec364c44fc0f40470bcda9efba",
    ),
    (
        "jetbrains-mono",
        "latin-700-italic",
        "2cd42fe8f872ef7e06156fe04338b23fc4ec532e66450c8e931db43473c0306c",
    ),
    (
        "jetbrains-mono",
        "latin-ext-400-normal",
        "505dfba8ecbe77e82765f36d317ed7ef4ac42719dc5f4ae68d1c483fd22d0d14",
    ),
    (
        "jetbrains-mono",
        "latin-ext-700-normal",
        "45a606d88f72bc0afae4d1a6988e3dba3a5e77ee51aa3b550e37c31c728f4d2d",
    ),
    (
        "jetbrains-mono",
        "latin-ext-400-italic",
        "a53b6b29b1621cdf51140761869e160521fc566a4a9f312d9b92e7178d433bc7",
    ),
    (
        "jetbrains-mono",
        "latin-ext-700-italic",
        "ebfe512fe16186a55f5576fec93d877718e25f90be5ad12223ea624498c33b1e",
    ),
];

/// The file is the one known for this font and name.
fn known(id: &str, name: &str, bytes: &[u8]) -> bool {
    HASHES
        .iter()
        .any(|&(i, n, hash)| i == id && n == name && app_core::blobs::sha256_hex(bytes) == hash)
}

fn invalid(what: &str) -> CommandError {
    CommandError::new("invalid", format!("not a font {what}"))
}

/// A Fontsource id: lowercase letters, digits and dashes.
fn checked_id(id: &str) -> CommandResult<&str> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if ok { Ok(id) } else { Err(invalid("id")) }
}

fn checked_file(name: &str) -> CommandResult<&str> {
    FILES
        .contains(&name)
        .then_some(name)
        .ok_or_else(|| invalid("file"))
}

/// The files asked for, [`FILES`] when the page names none. The first is the
/// one a font cannot do without.
fn checked_files(files: Option<Vec<String>>) -> CommandResult<Vec<String>> {
    let files = files.unwrap_or_else(|| FILES.map(str::to_owned).to_vec());
    if files.is_empty() || files.len() > FILES.len() {
        return Err(invalid("file list"));
    }
    for name in &files {
        checked_file(name)?;
    }
    Ok(files)
}

/// `fonts/<id>`.
fn folder(state: &AppState, id: &str) -> CommandResult<PathBuf> {
    Ok(state.dir.join("fonts").join(checked_id(id)?))
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
                id
            );
            let fetched = match fetch(&agent, &mirrored) {
                Ok(Some(bytes)) if known(&id, name, &bytes) => Ok(Some(bytes)),
                // not mirrored, or the host does not answer: Fontsource's own
                _ => fetch(
                    &agent,
                    &format!(
                        "{}/{id}@{FONTSOURCE}/{name}.woff2",
                        app_brand::FONTS_SOURCE_URL
                    ),
                ),
            };
            let bytes = match fetched {
                Ok(Some(bytes)) => bytes,
                // the first file is needed, the others can wait
                Err(err) if *name == files[0] => return Err(err),
                // a font without italics or the extended latin, or a file
                // that did not come this time
                Ok(None) | Err(_) => continue,
            };
            if !known(&id, name, &bytes) {
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
        .filter(|name| {
            let path = dir.join(format!("{name}.woff2"));
            let Ok(bytes) = std::fs::read(&path) else {
                return false;
            };
            // one kept before the files were checked, or changed since, goes
            // and is downloaded again
            let good = known(&id, name, &bytes);
            if !good {
                let _ = std::fs::remove_file(&path);
            }
            good
        })
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
    let bytes = std::fs::read(&path).map_err(|e| CommandError::new("notFound", e.to_string()))?;
    if !known(&id, &name, &bytes) {
        let _ = std::fs::remove_file(&path);
        return Err(CommandError::new(
            "notFound",
            "the font file is not the known one",
        ));
    }
    Ok(Response::new(bytes))
}

#[cfg(test)]
mod tests {
    use super::{FILES, HASHES, checked_file, checked_files, checked_id, known};

    #[test]
    fn only_the_known_files_are_kept() {
        assert!(!known("opendyslexic", "latin-400-normal", b"wOF2 anything"));
        assert!(!known("opendyslexic", "latin-400-normal", b""));
        // every name with a hash is one the app asks for
        assert!(HASHES.iter().all(|(_, name, _)| FILES.contains(name)));
    }

    #[test]
    fn only_plain_ids_and_known_files() {
        assert!(checked_id("jetbrains-mono").is_ok());
        assert!(checked_id("source-serif-4").is_ok());
        let long = "a".repeat(65);
        for bad in [
            "", "../x", "a/b", "A", "x y", "a.b", ":vf", "a:vf:vf", "a:x", &long,
        ] {
            assert!(checked_id(bad).is_err(), "{bad}");
        }
        assert!(checked_file("latin-400-normal").is_ok());
        assert!(checked_file("../latin-400-normal").is_err());
        assert!(checked_file("latin-900-normal").is_err());
        assert!(checked_file("latin-ext-wght-italic").is_err());
    }

    #[test]
    fn file_lists_default_to_the_usual_files_and_stay_known() {
        assert_eq!(checked_files(None).unwrap(), FILES.to_vec());
        let one = vec!["latin-700-normal".to_owned()];
        assert_eq!(checked_files(Some(one.clone())).unwrap(), one);
        assert!(checked_files(Some(vec![])).is_err());
        assert!(checked_files(Some(vec!["latin-400-normal".into(), "x".into()])).is_err());
        let too_many = vec!["latin-400-normal".to_owned(); FILES.len() + 1];
        assert!(checked_files(Some(too_many)).is_err());
    }
}
