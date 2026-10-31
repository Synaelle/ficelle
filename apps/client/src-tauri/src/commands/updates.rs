// SPDX-License-Identifier: AGPL-3.0-or-later

//! The app updating itself. Only numbered release builds do: a dev or
//! preview build has no version to compare. The newest GitHub release carries
//! tauri's `latest.json`, one entry per platform with the file's address and
//! its minisign signature, made with the key in `brand.json` (`appUpdatesKey`).
//! The file is downloaded here and checked against that key.
//!
//! On Windows the checked installer then runs and the app leaves; on Linux
//! the AppImage replaces itself and starts again. On Android the APK is kept
//! in `cache/updates/` for `AppInstaller` (MainActivity.kt) to hand to the
//! system installer, which asks the person.
//!
//! The portable Windows exe (`crate::portable`) only reads the list's
//! version and offers the release page, where the new exe is.

use serde::Serialize;

use crate::error::{CommandError, CommandResult};

/// A newer version, downloaded and ready to install.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdate {
    version: String,
    notes: String,
    /// The portable exe: not downloaded, this page has it.
    #[serde(skip_serializing_if = "Option::is_none")]
    page: Option<String>,
    /// It fixes a critical security problem: the app asks in the middle of
    /// the screen instead of a card in the corner. It never installs alone.
    critical: bool,
}

/// This build updates itself: a numbered version and an update key.
fn enabled() -> bool {
    !app_brand::APP_UPDATES_KEY.is_empty()
        && app_protocol::releases::version_parts(app_brand::VERSION).is_some()
}

fn failed(kind: &'static str, err: impl std::fmt::Display) -> CommandError {
    CommandError::new(kind, err.to_string())
}

mod list {
    //! Reading `latest.json`, downloading and checking a file.

    use std::collections::HashMap;
    use std::io::Read;
    use std::time::Duration;

    use base64::{Engine, engine::general_purpose::STANDARD};
    use minisign_verify::{PublicKey, Signature};
    use serde::Deserialize;

    use super::failed;
    use crate::error::CommandResult;

    const MAX_LIST: u64 = 1024 * 1024;
    const MAX_FILE: u64 = 300 * 1024 * 1024;

    /// `latest.json`, the parts read here.
    #[derive(Debug, Deserialize)]
    pub struct Latest {
        pub version: String,
        #[serde(default)]
        pub notes: String,
        /// The release fixes a critical security problem (its `security`
        /// flag in the signed release list).
        #[serde(default)]
        pub critical: bool,
        /// The portable exe only reads the version.
        #[serde(default)]
        pub platforms: HashMap<String, Platform>,
    }

    /// One platform's file.
    #[derive(Debug, Deserialize)]
    pub struct Platform {
        /// Base64 of the minisign signature file, as tauri writes it.
        pub signature: String,
        pub url: String,
    }

    /// `major.minor.micro`, a leading `v` allowed.
    fn parts(version: &str) -> Option<(u64, u64, u64)> {
        app_protocol::releases::version_parts(version.trim().trim_start_matches('v'))
    }

    /// The list's version is newer than `current`.
    pub fn newer(latest: &str, current: &str) -> bool {
        matches!((parts(latest), parts(current)), (Some(l), Some(c)) if l > c)
    }

    /// `latest.json`'s key for this device: `windows-x86_64`,
    /// `linux-x86_64`, `android-aarch64`, `android-arm`, `android-x86_64`
    /// (the last three the same APK).
    pub fn platform_key() -> String {
        format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
    }

    /// The version without a leading `v`, as file names and the ui use it.
    pub fn clean(version: &str) -> &str {
        version.trim().trim_start_matches('v')
    }

    /// Downloads `url`, refused past `max` bytes.
    fn get(url: &str, max: u64) -> CommandResult<Vec<u8>> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(600)))
            // the name alone: an outside host has no need of the version
            .user_agent(app_brand::SLUG)
            .build()
            .into();
        let response = agent.get(url).call().map_err(|e| failed("offline", e))?;
        let mut bytes = Vec::new();
        response
            .into_body()
            .into_reader()
            .take(max + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| failed("offline", e))?;
        if bytes.len() as u64 > max {
            return Err(failed("update", "the download is larger than it may be"));
        }
        Ok(bytes)
    }

    /// The list at this build's own address.
    pub fn latest() -> CommandResult<Latest> {
        serde_json::from_slice(&get(app_brand::APP_UPDATES_URL, MAX_LIST)?)
            .map_err(|e| failed("update", format!("latest.json: {e}")))
    }

    /// `bytes` were signed by the key, both in tauri's base64 form. Returns
    /// the signature's trusted comment, signed with the file.
    pub fn verify(bytes: &[u8], signature: &str, key: &str) -> Result<String, String> {
        let text = |b64: &str, what: &str| {
            STANDARD
                .decode(b64.trim())
                .ok()
                .and_then(|raw| String::from_utf8(raw).ok())
                .ok_or_else(|| format!("the {what} is not base64 text"))
        };
        let key = PublicKey::decode(&text(key, "update key")?)
            .map_err(|e| format!("the update key: {e}"))?;
        let signature = Signature::decode(&text(signature, "signature")?)
            .map_err(|e| format!("the signature: {e}"))?;
        // tauri signs a hash of the file, which minisign calls legacy-free
        // prehashed; plain signatures are accepted too, as tauri's updater does
        key.verify(bytes, &signature, true)
            .map_err(|_| "the file does not match its signature".to_owned())?;
        Ok(signature.trusted_comment().to_owned())
    }

    /// Whether a signature's trusted comment names `version`: its
    /// `version:` field (tauri's bundler writes one), else the version in
    /// its `file:` name (`Ficelle_0.3.0_x64-setup.exe`,
    /// `ficelle-0.3.0-android.apk`). An old file, signed for its own version,
    /// cannot be passed off as a newer one.
    pub fn signed_for(comment: &str, version: &str) -> bool {
        let version = clean(version);
        let field = |name: &str| {
            comment
                .split('\t')
                .find_map(|f| f.trim().strip_prefix(name))
                .map(str::trim)
        };
        if let Some(signed) = field("version:") {
            return clean(signed) == version;
        }
        field("file:").is_some_and(|name| name.split(['_', '-']).any(|part| part == version))
    }

    /// This device's file from the list, downloaded and checked: signed by
    /// the update key, for the version the list announces.
    pub fn download(file: &Platform, version: &str) -> CommandResult<Vec<u8>> {
        let bytes = get(&file.url, MAX_FILE)?;
        let comment = verify(&bytes, &file.signature, app_brand::APP_UPDATES_KEY)
            .map_err(|e| failed("update", e))?;
        if !signed_for(&comment, version) {
            return Err(failed("update", "the file is signed for another version"));
        }
        Ok(bytes)
    }
}

#[cfg(desktop)]
mod platform {
    //! Downloaded and checked here, then the installer (Windows) or the
    //! AppImage itself (Linux) takes over.

    use std::sync::Mutex;

    use tauri::{AppHandle, Manager};

    use super::list::{clean, download, latest, newer, platform_key};
    use super::{AppUpdate, failed};
    use crate::error::CommandResult;

    /// A checked update.
    struct Ready {
        version: String,
        notes: String,
        critical: bool,
        bytes: Vec<u8>,
    }

    /// The update downloaded and waiting for a restart.
    #[derive(Default)]
    pub struct Pending(Mutex<Option<Ready>>);

    fn info(ready: &Ready) -> AppUpdate {
        AppUpdate {
            version: ready.version.clone(),
            notes: ready.notes.clone(),
            page: None,
            critical: ready.critical,
        }
    }

    pub fn check(app: &AppHandle) -> CommandResult<Option<AppUpdate>> {
        let pending = app.state::<Pending>();
        if let Some(ready) = pending.0.lock().map_err(|e| failed("unknown", e))?.as_ref() {
            return Ok(Some(info(ready)));
        }
        let list = latest()?;
        if !newer(&list.version, app_brand::VERSION) {
            return Ok(None);
        }
        let Some(file) = list.platforms.get(&platform_key()) else {
            return Ok(None);
        };
        let ready = Ready {
            version: clean(&list.version).to_owned(),
            notes: list.notes.clone(),
            critical: list.critical,
            bytes: download(file, &list.version)?,
        };
        let found = info(&ready);
        *pending.0.lock().map_err(|e| failed("unknown", e))? = Some(ready);
        Ok(Some(found))
    }

    pub fn install(app: &AppHandle) -> CommandResult<()> {
        let taken = app
            .state::<Pending>()
            .0
            .lock()
            .map_err(|e| failed("unknown", e))?
            .take();
        let Some(ready) = taken else {
            return Err(failed("notFound", "no update is waiting"));
        };
        replace(app, &ready)
    }

    /// The installer, silent (no window, no questions), as an update,
    /// starting the app after. The app leaves at once so its files can be
    /// replaced.
    #[cfg(windows)]
    fn replace(_app: &AppHandle, ready: &Ready) -> CommandResult<()> {
        use std::process::Command;

        let dir = std::env::temp_dir().join(format!("{}-update", app_brand::SLUG));
        std::fs::create_dir_all(&dir).map_err(|e| failed("file", e))?;
        let setup = dir.join(format!("{}_{}_setup.exe", app_brand::SLUG, ready.version));
        std::fs::write(&setup, &ready.bytes).map_err(|e| failed("file", e))?;
        // read back right before it starts: the file checked is the one run
        let written = std::fs::read(&setup).map_err(|e| failed("file", e))?;
        if written != ready.bytes {
            let _ = std::fs::remove_file(&setup);
            return Err(failed(
                "update",
                "the installer changed after it was checked",
            ));
        }
        let args = ["/S", "/UPDATE", "/R", "/ARGS"];
        match Command::new(&setup).args(args).spawn() {
            Ok(_) => {}
            // an installer that asks for administrator rights only starts
            // through the shell, which shows the prompt
            Err(e) if e.raw_os_error() == Some(740) => {
                use std::os::windows::process::CommandExt;
                const NO_WINDOW: u32 = 0x0800_0000;
                Command::new("cmd")
                    .raw_arg(format!(
                        "/C start \"\" \"{}\" {}",
                        setup.display(),
                        args.join(" ")
                    ))
                    .creation_flags(NO_WINDOW)
                    .spawn()
                    .map_err(|e| failed("update", e))?;
            }
            Err(e) => return Err(failed("update", e)),
        }
        std::process::exit(0)
    }

    /// The AppImage the app runs from, replaced in place: written beside it
    /// first, then renamed over it, which the running app does not mind.
    #[cfg(target_os = "linux")]
    fn replace(app: &AppHandle, ready: &Ready) -> CommandResult<()> {
        use std::path::PathBuf;

        let Some(current) = std::env::var_os("APPIMAGE").map(PathBuf::from) else {
            return Err(failed(
                "unsupported",
                "only the AppImage updates itself, a package updates with the system",
            ));
        };
        // an AppImage is an ELF program
        if !ready.bytes.starts_with(b"\x7fELF") {
            return Err(failed("update", "the download is not an AppImage"));
        }
        let name = current
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let next = current.with_file_name(format!(".{name}.update"));
        let permissions = std::fs::metadata(&current)
            .map_err(|e| failed("file", e))?
            .permissions();
        std::fs::write(&next, &ready.bytes).map_err(|e| failed("file", e))?;
        let swapped = std::fs::set_permissions(&next, permissions)
            .and_then(|()| std::fs::rename(&next, &current));
        if let Err(e) = swapped {
            let _ = std::fs::remove_file(&next);
            return Err(failed("file", e));
        }
        app.restart()
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    fn replace(_app: &AppHandle, _ready: &Ready) -> CommandResult<()> {
        Err(failed("unsupported", "this system does not update itself"))
    }
}

#[cfg(target_os = "android")]
mod platform {
    //! Downloaded and checked here, installed by the system installer.

    use std::fs;
    use std::path::PathBuf;

    use tauri::{AppHandle, Manager};

    use super::list::{clean, download, latest, newer, platform_key};
    use super::{AppUpdate, failed};
    use crate::error::CommandResult;

    /// `cache/updates`, which `AppInstaller` reads.
    fn folder(app: &AppHandle) -> CommandResult<PathBuf> {
        let dir = app
            .path()
            .app_cache_dir()
            .map_err(|e| failed("file", e))?
            .join("updates");
        fs::create_dir_all(&dir).map_err(|e| failed("file", e))?;
        Ok(dir)
    }

    pub fn check(app: &AppHandle) -> CommandResult<Option<AppUpdate>> {
        let dir = folder(app)?;
        let latest = latest()?;
        let current = newer(&latest.version, app_brand::VERSION);
        let name = format!("{}-{}.apk", app_brand::SLUG, clean(&latest.version));
        // only the newest file stays, an installed or older one goes
        for entry in fs::read_dir(&dir).map_err(|e| failed("file", e))?.flatten() {
            if !current || entry.file_name().to_string_lossy() != name {
                let _ = fs::remove_file(entry.path());
            }
        }
        if !current {
            return Ok(None);
        }
        let found = AppUpdate {
            version: clean(&latest.version).to_owned(),
            notes: latest.notes.clone(),
            page: None,
            critical: latest.critical,
        };
        if dir.join(&name).is_file() {
            return Ok(Some(found));
        }
        let Some(file) = latest.platforms.get(&platform_key()) else {
            return Ok(None);
        };
        let bytes = download(file, &latest.version)?;
        // written under another name first: a file named like an update has
        // always been checked
        let part = dir.join(format!("{name}.part"));
        fs::write(&part, &bytes).map_err(|e| failed("file", e))?;
        fs::rename(&part, dir.join(&name)).map_err(|e| failed("file", e))?;
        Ok(Some(found))
    }
}

#[cfg(windows)]
mod portable {
    //! The portable exe: `latest.json` read for its version only, nothing
    //! downloaded, the release page has the new exe.

    use super::list::{clean, latest, newer};
    use super::{AppUpdate, failed};
    use crate::error::CommandResult;

    /// The release page the list belongs to, `.../releases/latest`.
    fn page() -> String {
        let url = app_brand::APP_UPDATES_URL;
        url.find("/download/")
            .map_or(url, |end| &url[..end])
            .to_owned()
    }

    pub fn check() -> CommandResult<Option<AppUpdate>> {
        let latest = latest()?;
        Ok(
            newer(&latest.version, app_brand::VERSION).then(|| AppUpdate {
                version: clean(&latest.version).to_owned(),
                notes: latest.notes,
                page: Some(page()),
                critical: latest.critical,
            }),
        )
    }

    /// Opens the release page in the browser. The address is this build's
    /// own, never one read from the list.
    pub fn open() -> CommandResult<()> {
        std::process::Command::new("explorer")
            .arg(page())
            .spawn()
            .map(drop)
            .map_err(|e| failed("unknown", e))
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn the_page_is_the_release_the_list_belongs_to() {
            assert!(
                super::page().ends_with("/releases/latest"),
                "{}",
                super::page()
            );
        }
    }
}

/// Looks for a newer version and downloads it. `None`: up to date, or this
/// build does not update itself.
#[tauri::command]
pub async fn app_update_check(app: tauri::AppHandle) -> CommandResult<Option<AppUpdate>> {
    if !enabled() {
        return Ok(None);
    }
    #[cfg(windows)]
    if crate::portable::PORTABLE {
        return tauri::async_runtime::spawn_blocking(portable::check)
            .await
            .map_err(|e| failed("unknown", e))?;
    }
    #[cfg(any(desktop, target_os = "android"))]
    return tauri::async_runtime::spawn_blocking(move || platform::check(&app))
        .await
        .map_err(|e| failed("unknown", e))?;
    #[cfg(target_os = "ios")]
    {
        let _ = app;
        Ok(None)
    }
}

/// The newest version on the release list, downloading nothing. With
/// automatic updates off, the app looks only to tell of a critical security
/// fix; the person chooses whether to update.
#[tauri::command]
pub async fn app_update_latest() -> CommandResult<Option<AppUpdate>> {
    if !enabled() {
        return Ok(None);
    }
    tauri::async_runtime::spawn_blocking(|| {
        let latest = list::latest()?;
        Ok(
            list::newer(&latest.version, app_brand::VERSION).then(|| AppUpdate {
                version: list::clean(&latest.version).to_owned(),
                notes: latest.notes,
                page: None,
                critical: latest.critical,
            }),
        )
    })
    .await
    .map_err(|e| failed("unknown", e))?
}

/// Desktop: installs the downloaded update and starts the app again.
#[tauri::command]
#[cfg_attr(
    desktop,
    expect(
        clippy::needless_pass_by_value,
        reason = "tauri hands commands the handle by value"
    )
)]
pub fn app_update_install(app: tauri::AppHandle) -> CommandResult<()> {
    #[cfg(windows)]
    if crate::portable::PORTABLE {
        return portable::open();
    }
    #[cfg(desktop)]
    return platform::install(&app);
    #[cfg(not(desktop))]
    {
        drop(app);
        Err(failed(
            "unsupported",
            "the system installer opens from the page",
        ))
    }
}

#[cfg(desktop)]
pub use platform::Pending;

#[cfg(test)]
mod tests {
    use super::list::{Latest, clean, newer, platform_key, signed_for, verify};

    #[test]
    fn reads_tauris_list() {
        let latest: Latest = serde_json::from_str(
            r#"{"version":"v0.1.2","notes":"Fixes","pub_date":"2026-11-01T00:00:00Z",
            "platforms":{"windows-x86_64":{"signature":"s","url":"https://example.org/a.exe"},
            "android-aarch64":{"signature":"t","url":"https://example.org/a.apk"}}}"#,
        )
        .unwrap();
        assert_eq!(clean(&latest.version), "0.1.2");
        assert_eq!(latest.notes, "Fixes");
        assert_eq!(
            latest.platforms["android-aarch64"].url,
            "https://example.org/a.apk"
        );
        assert_eq!(latest.platforms["android-aarch64"].signature, "t");
        assert_eq!(latest.platforms["windows-x86_64"].signature, "s");
        #[cfg(all(windows, target_arch = "x86_64"))]
        assert_eq!(platform_key(), "windows-x86_64");
    }

    #[test]
    fn newer_compares_numbers_only() {
        assert!(newer("0.1.1", "0.1.0"));
        assert!(newer("v0.2.0", "0.1.9"));
        assert!(newer("0.10.0", "0.9.0"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("0.1.0", "0.1.1"));
        assert!(!newer("0.1.1", "preview-abc"));
        assert!(!newer("latest", "0.1.0"));
    }

    // signed with a throwaway key by `tauri signer sign`
    const KEY: &str = include_str!("../../tests/update-key.pub");
    const SIGNATURE: &str = include_str!("../../tests/update-file.sig");
    const FILE: &[u8] = include_bytes!("../../tests/update-file.bin");

    #[test]
    fn a_signed_file_passes() {
        verify(FILE, SIGNATURE, KEY).unwrap();
    }

    #[test]
    fn the_signed_comment_names_the_version() {
        let bundler = "timestamp:1793355502\tfile:Ficelle_0.2.2_x64-setup.exe\tversion:0.2.2";
        assert!(signed_for(bundler, "0.2.2"));
        assert!(signed_for(bundler, "v0.2.2"));
        assert!(
            !signed_for(bundler, "99.0.0"),
            "an old file for a newer version"
        );
        let signer = "timestamp:1\tfile:ficelle-0.2.2-arm64.apk";
        assert!(signed_for(signer, "0.2.2"));
        assert!(!signed_for(signer, "0.2.3"));
        assert!(signed_for(
            "timestamp:1\tfile:ficelle-0.3.0-android.apk",
            "0.3.0"
        ));
        assert!(signed_for(
            "timestamp:1\tfile:Ficelle_0.2.2_amd64.AppImage",
            "0.2.2"
        ));
        assert!(!signed_for("timestamp:1\tfile:update-file.bin", "0.2.2"));
        assert!(!signed_for("", "0.2.2"));
        // the test file's own signature carries its comment back
        let comment = verify(FILE, SIGNATURE, KEY).unwrap();
        assert!(comment.contains("file:update-file.bin"), "{comment}");
    }

    #[test]
    fn a_changed_file_fails() {
        let mut changed = FILE.to_vec();
        changed[0] ^= 1;
        assert!(verify(&changed, SIGNATURE, KEY).is_err());
    }

    /// The public release's own file, read and checked the way the app does.
    #[cfg(windows)]
    #[test]
    #[ignore = "downloads the public release"]
    fn the_public_release_checks_out() {
        let latest = super::list::latest().unwrap();
        let file = &latest.platforms[&platform_key()];
        let bytes = super::list::download(file, &latest.version).unwrap();
        assert!(bytes.starts_with(b"MZ"), "a Windows program");
    }

    #[test]
    fn another_key_fails() {
        let other = app_brand::APP_UPDATES_KEY;
        assert!(verify(FILE, SIGNATURE, other).is_err());
        assert!(verify(FILE, SIGNATURE, "not base64").is_err());
        assert!(verify(FILE, "", KEY).is_err());
    }
}
