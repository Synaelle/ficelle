// SPDX-License-Identifier: AGPL-3.0-or-later

//! The app updating itself. Only numbered release builds do: a dev or
//! preview build has no version to compare. The newest GitHub release carries
//! tauri's `latest.json`, one entry per platform with the file's address and
//! its minisign signature, made with the key in `brand.json` (`appUpdatesKey`).
//! The file is downloaded here and checked against that key.
//!
//! On Windows the checked installer then runs and the app leaves; on Linux
//! the AppImage replaces itself and starts again; on a Mac the new `.app`
//! takes the running one's place and starts (`mac_app`). On Android the APK
//! is kept in `cache/updates/` for `AppInstaller` (MainActivity.kt) to hand to
//! the system installer, which asks the person.
//!
//! The portable Windows exe (`crate::portable`) only reads the list's version
//! and offers that version's release page, where the new file is. So does a
//! Mac app that cannot replace itself: still on its disk image, or moved
//! aside by macOS because it was opened where it was downloaded.
//!
//! Two lists: Stable's (`appUpdatesUrl`, GitHub's newest release, never a
//! pre-release) and Beta's (`appBetaUpdatesUrl`, on the release tagged
//! `beta`, which holds the newest of the betas and the stable versions). An
//! older version is only offered when asked for (`allow_older`): a beta
//! going back to Stable on purpose, never on a regular look. Going back also
//! needs the stable version to read the same database: the list's `schema`
//! (the number of database steps it knows, `devtool schema`) must equal this
//! build's, a list without one counts as different.

use serde::{Deserialize, Serialize};

use crate::error::{CommandError, CommandResult};

/// A newer version, downloaded and ready to install.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_excessive_bools,
    reason = "separate facts the ui reads, not a state"
)]
pub struct AppUpdate {
    version: String,
    notes: String,
    /// Not downloaded, this page has it: the portable exe, a Mac app that
    /// cannot replace itself or a release with no Mac file.
    #[serde(skip_serializing_if = "Option::is_none")]
    page: Option<String>,
    /// It fixes a critical security problem: the app asks in the middle of
    /// the screen instead of a card in the corner. It never installs alone.
    critical: bool,
    /// Older than this version: only found when asked for.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    older: bool,
    /// The version reads this build's database: the same schema steps.
    same_schema: bool,
    /// This build can install another version over itself: the installer,
    /// the AppImage, the APK (through an uninstall on Android).
    go_back: bool,
}

impl AppUpdate {
    fn new(latest: &list::Latest, page: Option<String>) -> Self {
        Self {
            version: list::clean(&latest.version).to_owned(),
            notes: latest.notes.clone(),
            page,
            critical: latest.critical,
            older: list::older(&latest.version, app_brand::VERSION),
            same_schema: list::same_schema(latest.schema),
            go_back: can_go_back(),
        }
    }
}

/// See [`AppUpdate::go_back`].
fn can_go_back() -> bool {
    if page_only() {
        return false;
    }
    cfg!(any(windows, target_os = "android"))
        || (cfg!(target_os = "linux") && std::env::var_os("APPIMAGE").is_some())
}

/// Which list the app follows, chosen in About.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    /// Tested versions, and small fixes in between.
    #[default]
    Stable,
    /// Early versions of the next release too.
    Beta,
}

impl Channel {
    /// The channel's `latest.json`.
    fn url(self) -> &'static str {
        match self {
            Self::Stable => app_brand::APP_UPDATES_URL,
            Self::Beta => app_brand::APP_BETA_UPDATES_URL,
        }
    }
}

/// Whether this version is for this app. Most releases have no Mac file, and a
/// Mac then hears nothing of them, not even a page without a Mac download.
fn for_this_app(latest: &list::Latest) -> bool {
    !cfg!(target_os = "macos") || latest.platforms.contains_key(&list::platform_key())
}

/// This build offers a release page instead of installing: the portable
/// exe, a Linux package, and a Mac app that cannot replace itself where it is.
#[cfg(target_os = "macos")]
fn page_only() -> bool {
    !mac_app::replaceable()
}

/// See the Mac's [`page_only`]. On Linux only the AppImage replaces itself,
/// a package (the AUR, Flathub) updates through the system.
#[cfg(not(target_os = "macos"))]
fn page_only() -> bool {
    crate::portable::PORTABLE
        || (cfg!(target_os = "linux") && std::env::var_os("APPIMAGE").is_none())
}

/// The repository's releases, `https://github.com/<owner>/<repo>/releases`,
/// from the address of an update list.
fn releases_in(url: &str) -> &str {
    url.find("/releases/")
        .map_or(url, |at| &url[..at + "/releases".len()])
}

/// The GitHub page of `version`'s release, in the repository the update
/// lists are in. `version` is one `newer` accepted: digits, dots and a
/// beta's number, nothing that could change the address.
fn release_page(version: &str) -> String {
    format!(
        "{}/tag/v{}",
        releases_in(app_brand::APP_UPDATES_URL),
        list::clean(version)
    )
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

    use super::{Channel, failed};
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
        /// How many database schema steps the version knows, `None` in
        /// lists made before it was written.
        #[serde(default)]
        pub schema: Option<usize>,
    }

    /// One platform's file.
    #[derive(Debug, Deserialize)]
    pub struct Platform {
        /// Base64 of the minisign signature file, as tauri writes it.
        pub signature: String,
        pub url: String,
    }

    /// `major.minor.micro` and a beta's number, a leading `v` allowed.
    fn parts(version: &str) -> Option<(u64, u64, u64, u64)> {
        app_protocol::releases::version_parts(version.trim().trim_start_matches('v'))
    }

    /// The list's version is newer than `current`.
    pub fn newer(latest: &str, current: &str) -> bool {
        matches!((parts(latest), parts(current)), (Some(l), Some(c)) if l > c)
    }

    /// The list's version is older than `current`.
    pub fn older(latest: &str, current: &str) -> bool {
        matches!((parts(latest), parts(current)), (Some(l), Some(c)) if l < c)
    }

    /// A version that reads this build's database: the same schema steps.
    /// An older app refuses a database a newer schema wrote.
    pub fn same_schema(schema: Option<usize>) -> bool {
        schema == Some(app_core::db::schema_version())
    }

    /// The list's version is offered: newer, or, asked for, older with the
    /// same schema. The same version never is.
    pub fn offered(latest: &Latest, current: &str, allow_older: bool) -> bool {
        newer(&latest.version, current)
            || (allow_older && older(&latest.version, current) && same_schema(latest.schema))
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

    /// The channel's list, at this build's own address.
    pub fn latest(channel: Channel) -> CommandResult<Latest> {
        serde_json::from_slice(&get(channel.url(), MAX_LIST)?)
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
        field("file:").is_some_and(|name| names_version(name, version))
    }

    /// `name` holds `version` whole, between the name's separators: a beta's
    /// own dash included (`ficelle-0.5.0-beta.1-android.apk`), and never the
    /// stable `0.5.0` read out of that beta's name.
    fn names_version(name: &str, version: &str) -> bool {
        name.match_indices(version).any(|(at, _)| {
            let before = name[..at].chars().next_back();
            let after = &name[at + version.len()..];
            matches!(before, Some('_' | '-'))
                && (after.starts_with(['_', '.'])
                    || (after.starts_with('-') && !after.starts_with("-beta.")))
        })
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

#[cfg(target_os = "macos")]
mod mac_app {
    //! The Mac app replaced in place, the way tauri's own updater does it: the
    //! checked archive unpacked beside the app, then two renames, which the
    //! running program does not mind. Everything happens on the app's own disk.

    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// `Ficelle.app` from `Ficelle.app/Contents/MacOS/ficelle`.
    fn bundle_of(program: &Path) -> Option<PathBuf> {
        let app = program.parent()?.parent()?.parent()?;
        (app.extension()? == "app").then(|| app.to_path_buf())
    }

    /// The `.app` this program runs from, `None` when run outside of one.
    pub fn running() -> Option<PathBuf> {
        bundle_of(&std::env::current_exe().ok()?)
    }

    /// A hidden folder next to the app, on the same disk.
    fn beside(app: &Path, what: &str) -> PathBuf {
        app.with_file_name(format!(".{}-{what}", app_brand::SLUG))
    }

    /// Something can be written next to `app`, and macOS has not moved it to
    /// its read-only copy of an app opened where it was downloaded.
    fn can_replace(app: &Path) -> bool {
        if app.to_string_lossy().contains("/AppTranslocation/") {
            return false;
        }
        let probe = beside(app, "probe");
        let made = fs::create_dir(&probe).is_ok();
        if made {
            let _ = fs::remove_dir(&probe);
        }
        made
    }

    /// The running app can be replaced where it is.
    pub fn replaceable() -> bool {
        running().is_some_and(|app| can_replace(&app))
    }

    /// `app` replaced by the `.app` in `archive`, a checked `.tar.gz`. On any
    /// failure the app stays as it was.
    pub fn replace(app: &Path, archive: &[u8]) -> Result<(), String> {
        let program = std::env::current_exe()
            .ok()
            .and_then(|path| path.file_name().map(ToOwned::to_owned))
            .ok_or("the program's name cannot be read")?;
        replace_holding(app, archive, Path::new(&program))
    }

    /// `program`: the new app must hold it, or it could not start again.
    fn replace_holding(app: &Path, archive: &[u8], program: &Path) -> Result<(), String> {
        let work = beside(app, "update");
        let old = beside(app, "old");
        // what an update cut short left behind
        let _ = fs::remove_dir_all(&work);
        let _ = fs::remove_dir_all(&old);
        fs::create_dir(&work).map_err(|e| format!("the update folder: {e}"))?;
        let done = unpack(&work, archive, program).and_then(|new| swap(app, &new, &old));
        let _ = fs::remove_dir_all(&work);
        // after a failed swap `old` may be the only copy of the app left
        if done.is_ok() {
            let _ = fs::remove_dir_all(&old);
        }
        done
    }

    /// The archive's one `.app`, unpacked in `work` by the system's own tar.
    fn unpack(work: &Path, archive: &[u8], program: &Path) -> Result<PathBuf, String> {
        let file = work.join("update.tar.gz");
        fs::write(&file, archive).map_err(|e| format!("the archive: {e}"))?;
        let unpacked = Command::new("/usr/bin/tar")
            .arg("-xzf")
            .arg(&file)
            .arg("-C")
            .arg(work)
            .output()
            .map_err(|e| format!("tar: {e}"))?;
        if !unpacked.status.success() {
            return Err(format!(
                "tar: {}",
                String::from_utf8_lossy(&unpacked.stderr).trim()
            ));
        }
        let mut apps = fs::read_dir(work)
            .map_err(|e| format!("the update folder: {e}"))?
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|e| e == "app"));
        let (Some(new), None) = (apps.next(), apps.next()) else {
            return Err("the archive does not hold one app".to_owned());
        };
        if !new.join("Contents/MacOS").join(program).is_file() {
            return Err("the app in the archive has no program".to_owned());
        }
        Ok(new)
    }

    /// Two renames: if the second fails the first is undone.
    fn swap(app: &Path, new: &Path, old: &Path) -> Result<(), String> {
        fs::rename(app, old).map_err(|e| format!("moving the app aside: {e}"))?;
        if let Err(e) = fs::rename(new, app) {
            return Err(match fs::rename(old, app) {
                Ok(()) => format!("putting the new app in place: {e}"),
                Err(back) => format!(
                    "putting the new app in place: {e}, and the app could not be put back, it is at {}: {back}",
                    old.display()
                ),
            });
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::{bundle_of, can_replace, replace_holding};
        use std::fs;
        use std::path::{Path, PathBuf};
        use std::process::Command;

        /// An empty folder of its own for each test.
        fn folder(name: &str) -> PathBuf {
            let dir =
                std::env::temp_dir().join(format!("ficelle-mac-app-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            dir
        }

        fn app_in(dir: &Path, name: &str, says: &str) -> PathBuf {
            let app = dir.join(name);
            fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
            fs::write(app.join("Contents/MacOS/ficelle"), says).unwrap();
            app
        }

        /// A `.tar.gz` of `dir`'s `what`.
        fn archive(dir: &Path, what: &str) -> Vec<u8> {
            let file = dir.join("made.tar.gz");
            let made = Command::new("/usr/bin/tar")
                .arg("-czf")
                .arg(&file)
                .arg("-C")
                .arg(dir)
                .arg(what)
                .status()
                .unwrap();
            assert!(made.success());
            fs::read(file).unwrap()
        }

        fn says(app: &Path) -> String {
            fs::read_to_string(app.join("Contents/MacOS/ficelle")).unwrap()
        }

        /// Nothing but the app stays next to it.
        fn alone(dir: &Path) {
            let names: Vec<_> = fs::read_dir(dir)
                .unwrap()
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            assert_eq!(names, ["Ficelle.app"], "{names:?}");
        }

        #[test]
        fn finds_the_app_around_the_program() {
            assert_eq!(
                bundle_of(Path::new(
                    "/Applications/Ficelle.app/Contents/MacOS/ficelle"
                )),
                Some(PathBuf::from("/Applications/Ficelle.app"))
            );
            assert_eq!(bundle_of(Path::new("/usr/local/bin/ficelle")), None);
        }

        #[test]
        fn the_new_app_takes_the_place_of_the_old() {
            let made = folder("swap-made");
            app_in(&made, "Ficelle.app", "new");
            let bytes = archive(&made, "Ficelle.app");
            let place = folder("swap-place");
            let app = app_in(&place, "Ficelle.app", "old");
            replace_holding(&app, &bytes, Path::new("ficelle")).unwrap();
            assert_eq!(says(&app), "new");
            alone(&place);
        }

        #[test]
        fn a_renamed_app_keeps_its_name() {
            let made = folder("renamed-made");
            app_in(&made, "Ficelle.app", "new");
            let bytes = archive(&made, "Ficelle.app");
            let place = folder("renamed-place");
            let app = app_in(&place, "Notes.app", "old");
            replace_holding(&app, &bytes, Path::new("ficelle")).unwrap();
            assert_eq!(says(&app), "new");
            assert!(!place.join("Ficelle.app").exists());
        }

        #[test]
        fn a_bad_archive_leaves_the_app_as_it_was() {
            let place = folder("bad-place");
            let app = app_in(&place, "Ficelle.app", "old");
            assert!(replace_holding(&app, b"not an archive", Path::new("ficelle")).is_err());
            assert_eq!(says(&app), "old");
            alone(&place);
        }

        #[test]
        fn an_archive_without_the_program_is_refused() {
            let made = folder("empty-made");
            fs::create_dir_all(made.join("Ficelle.app/Contents/MacOS")).unwrap();
            let no_program = archive(&made, "Ficelle.app");
            let other = folder("empty-other");
            fs::write(other.join("readme.txt"), "x").unwrap();
            let no_app = archive(&other, "readme.txt");
            let place = folder("empty-place");
            let app = app_in(&place, "Ficelle.app", "old");
            for bytes in [no_program, no_app] {
                assert!(replace_holding(&app, &bytes, Path::new("ficelle")).is_err());
                assert_eq!(says(&app), "old");
            }
            alone(&place);
        }

        #[test]
        fn what_a_cut_update_left_is_cleared() {
            let made = folder("left-made");
            app_in(&made, "Ficelle.app", "new");
            let bytes = archive(&made, "Ficelle.app");
            let place = folder("left-place");
            let app = app_in(&place, "Ficelle.app", "old");
            fs::create_dir_all(super::beside(&app, "update").join("junk")).unwrap();
            fs::create_dir_all(super::beside(&app, "old").join("junk")).unwrap();
            replace_holding(&app, &bytes, Path::new("ficelle")).unwrap();
            assert_eq!(says(&app), "new");
            alone(&place);
        }

        #[test]
        fn cannot_replace_a_moved_or_read_only_app() {
            let place = folder("can");
            let app = app_in(&place, "Ficelle.app", "old");
            assert!(can_replace(&app));
            alone(&place);
            assert!(!can_replace(Path::new(
                "/private/var/folders/x/AppTranslocation/y/d/Ficelle.app"
            )));
            assert!(!can_replace(Path::new("/no/such/place/Ficelle.app")));
        }
    }
}

#[cfg(desktop)]
mod platform {
    //! Downloaded and checked here, then the installer (Windows), the AppImage
    //! itself (Linux) or the new app (Mac) takes over.

    use std::sync::Mutex;

    use tauri::{AppHandle, Manager};

    use super::list::{Latest, download, latest, offered, platform_key};
    use super::{AppUpdate, Channel, failed};
    use crate::error::CommandResult;

    /// A checked update.
    struct Ready {
        /// The list it came from: another channel chosen since looks again.
        channel: Channel,
        info: AppUpdate,
        bytes: Vec<u8>,
    }

    /// The update downloaded and waiting for a restart.
    #[derive(Default)]
    pub struct Pending(Mutex<Option<Ready>>);

    #[cfg(target_os = "macos")]
    impl Pending {
        /// An update waits. Without one, the last look found only a page.
        pub fn waiting(&self) -> bool {
            self.0.lock().is_ok_and(|ready| ready.is_some())
        }
    }

    pub fn check(
        app: &AppHandle,
        channel: Channel,
        allow_older: bool,
    ) -> CommandResult<Option<AppUpdate>> {
        let pending = app.state::<Pending>();
        {
            let mut waiting = pending.0.lock().map_err(|e| failed("unknown", e))?;
            match waiting.as_ref() {
                // an older one waits only for the look that asked for it
                Some(ready) if ready.channel == channel && (allow_older || !ready.info.older) => {
                    return Ok(Some(ready.info.clone()));
                }
                // a beta waiting when Stable is chosen is never installed
                Some(_) => *waiting = None,
                None => {}
            }
        }
        let list: Latest = latest(channel)?;
        if !offered(&list, app_brand::VERSION, allow_older) {
            return Ok(None);
        }
        let Some(file) = list.platforms.get(&platform_key()) else {
            return Ok(None);
        };
        let ready = Ready {
            channel,
            info: AppUpdate::new(&list, None),
            bytes: download(file, &list.version)?,
        };
        let found = ready.info.clone();
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
        let setup = dir.join(format!(
            "{}_{}_setup.exe",
            app_brand::SLUG,
            ready.info.version
        ));
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

    /// The new app in the running one's place, then started. The list's file
    /// was checked against the update key when it was downloaded.
    #[cfg(target_os = "macos")]
    fn replace(app: &AppHandle, ready: &Ready) -> CommandResult<()> {
        let Some(current) = super::mac_app::running() else {
            return Err(failed("unsupported", "this program is not in an app"));
        };
        super::mac_app::replace(&current, &ready.bytes).map_err(|e| failed("update", e))?;
        app.restart()
    }

    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
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

    use super::list::{clean, download, latest, offered, platform_key};
    use super::{AppUpdate, Channel, failed};
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

    /// With `allow_older`, an older APK is kept too, for `Installer` to copy
    /// to Downloads: Android installs it only after an uninstall.
    pub fn check(
        app: &AppHandle,
        channel: Channel,
        allow_older: bool,
    ) -> CommandResult<Option<AppUpdate>> {
        let dir = folder(app)?;
        let latest = latest(channel)?;
        let current = offered(&latest, app_brand::VERSION, allow_older);
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
        let found = AppUpdate::new(&latest, None);
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

#[cfg(desktop)]
mod page {
    //! The portable exe, a Linux package and a Mac app that cannot update
    //! itself: `latest.json`
    //! read for its version only, nothing downloaded, the version's release
    //! page has the file.

    use std::sync::Mutex;

    use super::list::{latest, newer};
    use super::{AppUpdate, Channel, failed, for_this_app, release_page, releases_in};
    use crate::error::CommandResult;

    /// The version the last look found, whose page opens.
    static FOUND: Mutex<Option<String>> = Mutex::new(None);

    pub fn check(channel: Channel) -> CommandResult<Option<AppUpdate>> {
        let latest = latest(channel)?;
        let found = (newer(&latest.version, app_brand::VERSION) && for_this_app(&latest))
            .then(|| AppUpdate::new(&latest, Some(release_page(&latest.version))));
        *FOUND.lock().map_err(|e| failed("unknown", e))? =
            found.as_ref().map(|update| update.version.clone());
        Ok(found)
    }

    /// Opens the found version's release page in the browser, or the list
    /// of releases when none was found. The address is this build's own,
    /// only the checked version comes from the list.
    pub fn open() -> CommandResult<()> {
        let found = FOUND.lock().map_err(|e| failed("unknown", e))?.clone();
        let page = found.map_or_else(
            || releases_in(app_brand::APP_UPDATES_URL).to_owned(),
            |version| release_page(&version),
        );
        let opener = if cfg!(windows) {
            "explorer"
        } else if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        std::process::Command::new(opener)
            .arg(page)
            .spawn()
            .map(drop)
            .map_err(|e| failed("unknown", e))
    }
}

/// This build only points to the release page, see [`page_only`]: the page
/// says so instead of how updates download.
#[tauri::command]
#[must_use]
pub fn app_update_page_only() -> bool {
    page_only()
}

/// Looks for a newer version and downloads it. `None`: up to date, or this
/// build does not update itself. `allow_older`: a beta going back to Stable,
/// an older version with the same schema downloads too (never on the
/// portable exe, which only reads the version, nor on the Mac, which only
/// follows Stable).
#[tauri::command]
pub async fn app_update_check(
    app: tauri::AppHandle,
    channel: Channel,
    allow_older: Option<bool>,
) -> CommandResult<Option<AppUpdate>> {
    if !enabled() {
        return Ok(None);
    }
    let allow_older = allow_older.unwrap_or(false);
    #[cfg(desktop)]
    if page_only() {
        return tauri::async_runtime::spawn_blocking(move || page::check(channel))
            .await
            .map_err(|e| failed("unknown", e))?;
    }
    tauri::async_runtime::spawn_blocking(move || platform::check(&app, channel, allow_older))
        .await
        .map_err(|e| failed("unknown", e))?
}

/// The newest version on the release list, downloading nothing. With
/// automatic updates off, the app looks only to tell of a critical security
/// fix; the person chooses whether to update. `allow_older`: an older
/// version is returned too, schema or not, for a beta to tell whether it can
/// go back to Stable (`older`, `sameSchema`, `goBack`).
#[tauri::command]
pub async fn app_update_latest(
    channel: Channel,
    allow_older: Option<bool>,
) -> CommandResult<Option<AppUpdate>> {
    if !enabled() {
        return Ok(None);
    }
    let allow_older = allow_older.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || {
        let latest = list::latest(channel)?;
        let shown = for_this_app(&latest)
            && (list::newer(&latest.version, app_brand::VERSION)
                || (allow_older && list::older(&latest.version, app_brand::VERSION)));
        Ok(shown
            .then(|| AppUpdate::new(&latest, page_only().then(|| release_page(&latest.version)))))
    })
    .await
    .map_err(|e| failed("unknown", e))?
}

/// Desktop: installs the downloaded update and starts the app again. The
/// portable exe, and a Mac app with nothing downloaded, open the release page
/// instead.
#[tauri::command]
#[cfg_attr(
    desktop,
    expect(
        clippy::needless_pass_by_value,
        reason = "tauri hands commands the handle by value"
    )
)]
pub fn app_update_install(app: tauri::AppHandle) -> CommandResult<()> {
    #[cfg(desktop)]
    if page_only() {
        return page::open();
    }
    #[cfg(target_os = "macos")]
    {
        use tauri::Manager;
        if !app.state::<platform::Pending>().waiting() {
            return page::open();
        }
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
    use super::list::{Latest, clean, newer, offered, older, same_schema, signed_for, verify};
    use super::{Channel, release_page, releases_in};

    fn list(version: &str, schema: Option<usize>) -> Latest {
        Latest {
            version: version.to_owned(),
            notes: String::new(),
            critical: false,
            platforms: std::collections::HashMap::new(),
            schema,
        }
    }

    #[test]
    fn reads_the_schema() {
        let with: Latest =
            serde_json::from_str(r#"{"version":"0.4.1","schema":18,"platforms":{}}"#).unwrap();
        assert_eq!(with.schema, Some(18));
        let without: Latest = serde_json::from_str(r#"{"version":"0.3.2"}"#).unwrap();
        assert_eq!(without.schema, None);
    }

    #[test]
    fn same_schema_needs_the_same_steps() {
        let here = app_core::db::schema_version();
        assert!(same_schema(Some(here)));
        assert!(!same_schema(Some(here - 1)));
        assert!(!same_schema(Some(here + 1)));
        // a list made before the field was written
        assert!(!same_schema(None));
    }

    #[test]
    fn an_older_version_only_when_asked() {
        let here = Some(app_core::db::schema_version());
        assert!(older("0.4.1", "0.5.0-beta.2"));
        assert!(!older("0.5.0", "0.5.0-beta.2"));
        assert!(!older("0.4.1", "0.4.1"));
        assert!(!older("latest", "0.4.1"));
        // newer is always offered, asked or not
        assert!(offered(&list("0.5.0", None), "0.5.0-beta.2", false));
        assert!(offered(&list("0.5.0", None), "0.5.0-beta.2", true));
        // older only when asked, and only with the same schema
        assert!(!offered(&list("0.4.1", here), "0.5.0-beta.2", false));
        assert!(offered(&list("0.4.1", here), "0.5.0-beta.2", true));
        assert!(!offered(&list("0.4.1", Some(1)), "0.5.0-beta.2", true));
        assert!(!offered(&list("0.4.1", None), "0.5.0-beta.2", true));
        // the same version never
        assert!(!offered(&list("0.4.1", here), "0.4.1", true));
    }

    #[test]
    fn the_channel_comes_from_the_ui() {
        let read = |text: &str| serde_json::from_str::<Channel>(text).ok();
        assert_eq!(read(r#""stable""#), Some(Channel::Stable));
        assert_eq!(read(r#""beta""#), Some(Channel::Beta));
        assert_eq!(read(r#""nightly""#), None);
        assert_eq!(Channel::default(), Channel::Stable);
    }

    #[test]
    fn each_channel_has_its_list() {
        assert_eq!(Channel::Stable.url(), app_brand::APP_UPDATES_URL);
        assert_eq!(Channel::Beta.url(), app_brand::APP_BETA_UPDATES_URL);
        assert_ne!(Channel::Stable.url(), Channel::Beta.url());
        // both lists live in the same repository, whose pages the app opens
        assert_eq!(
            releases_in(Channel::Stable.url()),
            releases_in(Channel::Beta.url())
        );
    }

    #[test]
    fn the_page_is_the_versions_release() {
        assert_eq!(
            releases_in("https://github.com/owner/repo/releases/latest/download/latest.json"),
            "https://github.com/owner/repo/releases"
        );
        assert_eq!(
            releases_in("https://github.com/owner/repo/releases/download/beta/latest.json"),
            "https://github.com/owner/repo/releases"
        );
        let releases = releases_in(app_brand::APP_UPDATES_URL);
        assert!(releases.starts_with("https://github.com/"), "{releases}");
        assert!(releases.ends_with("/releases"), "{releases}");
        assert_eq!(release_page("0.4.1"), format!("{releases}/tag/v0.4.1"));
        assert_eq!(
            release_page("v0.5.0-beta.2"),
            format!("{releases}/tag/v0.5.0-beta.2")
        );
    }

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
        assert_eq!(super::list::platform_key(), "windows-x86_64");
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
        // a beta reaches its release, and a release never goes back to a beta
        assert!(newer("0.4.0-beta.2", "0.4.0-beta.1"));
        assert!(newer("0.4.0", "0.4.0-beta.3"));
        assert!(newer("0.4.0-beta.1", "0.3.3"));
        assert!(!newer("0.4.0-beta.3", "0.4.0"));
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
        // a beta's version has a dash of its own
        let beta = "timestamp:1\tfile:ficelle-0.5.0-beta.1-android.apk";
        assert!(signed_for(beta, "0.5.0-beta.1"));
        assert!(
            !signed_for(beta, "0.5.0"),
            "a beta's file passed off as the release"
        );
        assert!(!signed_for(beta, "0.5.0-beta.2"));
        assert!(signed_for(
            "timestamp:1\tfile:Ficelle_0.5.0-beta.1_x64-setup.exe",
            "0.5.0-beta.1"
        ));
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
        let latest = super::list::latest(super::Channel::Stable).unwrap();
        let file = &latest.platforms[&super::list::platform_key()];
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
