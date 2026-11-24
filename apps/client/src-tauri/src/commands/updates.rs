// SPDX-License-Identifier: AGPL-3.0-or-later

//! The app updating itself. Only numbered release builds do: a dev or
//! preview build has no version to compare. The newest GitHub release carries
//! tauri's `latest.json`, one entry per platform with the file's address and
//! its minisign signature, made with the key in `brand.json` (`appUpdatesKey`).
//! The app only reads the list on its own. The file is downloaded when the
//! person asks for it, into a `.part` file a later try goes on from, with
//! its progress sent to the page (`app-update-progress`), then checked
//! against that key.
//!
//! On Windows the checked installer then runs and the app leaves; on Linux
//! the AppImage replaces itself and starts again. On Android the APK
//! is kept in `cache/updates/` for `AppInstaller` (MainActivity.kt) to hand to
//! the system installer, which asks the person.
//!
//! The portable Windows exe (`crate::portable`) only reads the list's version
//! and offers that version's release page, where the new file is.
//!
//! The Arch package (`tools/arch/PKGBUILD`, see [`pacman`]) has a file of its
//! own in the list, `linux-x86_64-pacman`, which older apps never read. It is
//! checked like the others, then installed with `pacman -U` through
//! `pkexec`, which shows the system's password window, and the app offers to
//! restart into it. It never takes the AppImage's way.
//!
//! Two lists: Stable's (`appUpdatesUrl`, GitHub's newest release, never a
//! pre-release) and Nightly's (`appNightlyUpdatesUrl`, on the release tagged
//! `nightly`, which holds the newest of the nightlies and the stable
//! versions, Linux and Android only). Versions are ordered by build number
//! (`list::rank`): a nightly's name holds none, and nightlies sit between
//! two releases, so the next release updates a nightly in place. An older
//! version is only offered when asked for (`allow_older`): going back to
//! Stable at once on purpose, never on a regular look. Going back also
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
    /// What the release is about, from a fixed set the app translates:
    /// `fixes`, or `platform:Linux` with what fills it in.
    messages: Vec<String>,
    /// The release's list of changes, in English.
    changelog: String,
    /// Not downloaded, this page has it: the portable exe, a Linux package.
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
            messages: latest.messages.clone(),
            changelog: latest.changelog.clone(),
            page,
            critical: latest.critical,
            older: list::older(latest, list::here()),
            same_schema: list::same_schema(latest.schema),
            go_back: can_go_back(),
        }
    }
}

/// See [`AppUpdate::go_back`].
fn can_go_back() -> bool {
    // pacman installs an older package too, with a warning
    if page_only() {
        return false;
    }
    cfg!(any(windows, target_os = "android"))
        || (cfg!(target_os = "linux") && (pacman() || std::env::var_os("APPIMAGE").is_some()))
}

/// Which list the app follows, chosen in About.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    /// Tested versions, and small fixes in between. Beta, which nightlies
    /// replaced, reads as Stable.
    #[default]
    #[serde(alias = "beta")]
    Stable,
    /// A build of each night's changes too, Linux and Android.
    Nightly,
}

impl Channel {
    /// The channel's `latest.json`.
    fn url(self) -> &'static str {
        match self {
            Self::Stable => app_brand::APP_UPDATES_URL,
            Self::Nightly => app_brand::APP_NIGHTLY_UPDATES_URL,
        }
    }
}

/// This build offers a release page instead of installing: the portable
/// exe, and a Linux package other than the Arch one. On Linux the AppImage
/// replaces itself and the Arch package updates through pacman, any other
/// package (the old AUR one, a Flatpak) through the system. A Flatpak is
/// never an AppImage, even when `APPIMAGE` came in from the starting
/// process's environment: its own files are read-only.
fn page_only() -> bool {
    crate::portable::PORTABLE
        || (cfg!(target_os = "linux")
            && !pacman()
            && (std::env::var_os("APPIMAGE").is_none() || std::env::var_os("FLATPAK_ID").is_some()))
}

/// This build was installed by the Arch package: it runs as
/// `/usr/bin/<slug>` and the package's mark is in `/usr/lib/<slug>`. Read
/// once, from the program the app started as (tauri keeps it), so an update
/// that replaced the file meanwhile changes nothing. An `APPIMAGE` in the
/// environment, passed on by an AppImage that started it, never counts.
pub(crate) fn pacman() -> bool {
    static PACMAN: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| {
        cfg!(target_os = "linux")
            && tauri::utils::platform::current_exe()
                .is_ok_and(|exe| installed_by_pacman(&exe, std::path::Path::new("/")))
    });
    *PACMAN
}

/// `exe` is the Arch package's program under `root`, its mark beside it.
fn installed_by_pacman(exe: &std::path::Path, root: &std::path::Path) -> bool {
    exe == root.join("usr/bin").join(app_brand::SLUG)
        && root
            .join("usr/lib")
            .join(app_brand::SLUG)
            .join("arch-package")
            .is_file()
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

/// This build updates itself: a release or a nightly, with an update key,
/// and not the APK for F-Droid, which updates it.
fn enabled() -> bool {
    !cfg!(app_store) && !app_brand::APP_UPDATES_KEY.is_empty() && list::here().is_some()
}

fn failed(kind: &'static str, err: impl std::fmt::Display) -> CommandError {
    CommandError::new(kind, err.to_string())
}

/// How far a download is, for the page.
#[derive(Debug, Clone, Copy, Serialize)]
struct Progress {
    done: u64,
    /// `None` when the server does not say the size.
    total: Option<u64>,
}

/// Sends a download's progress to the page, a few times a second at most.
fn reporter(app: &tauri::AppHandle) -> impl FnMut(u64, Option<u64>) {
    use std::time::{Duration, Instant};
    use tauri::Emitter;

    let app = app.clone();
    let mut sent: Option<Instant> = None;
    move |done, total| {
        if sent.is_some_and(|at| at.elapsed() < Duration::from_millis(200)) && Some(done) != total {
            return;
        }
        sent = Some(Instant::now());
        let _ = app.emit("app-update-progress", Progress { done, total });
    }
}

mod list {
    //! Reading `latest.json`, downloading and checking a file.

    use std::collections::HashMap;
    use std::fs::{self, OpenOptions};
    use std::io::{Read, Write};
    use std::path::Path;
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
        /// See [`super::AppUpdate::messages`]; lists made before have none.
        #[serde(default)]
        pub messages: Vec<String>,
        #[serde(default)]
        pub changelog: String,
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
        /// Where the version stands, see [`rank`]: given for a nightly,
        /// whose name holds no number, `None` in lists made before.
        #[serde(default)]
        pub build: Option<u64>,
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

    /// Where a version stands among releases and nightlies: the build
    /// number given (a nightly's, `nightly-2026-11-16` holds none), else the
    /// one its release version makes, Android's version code. Nightlies sit
    /// between two releases, so the next release is newer than each.
    pub fn rank(version: &str, build: Option<u64>) -> Option<u64> {
        build.filter(|b| *b > 0).or_else(|| {
            parts(version).map(|(major, minor, micro, beta)| {
                (major * 1000 + minor) * 100_000 + micro * 100 + beta.min(99)
            })
        })
    }

    /// This build's place, see [`rank`]. `None` for a dev or preview build.
    pub fn here() -> Option<u64> {
        rank(app_brand::VERSION, app_brand::BUILD.parse().ok())
    }

    /// A nightly's day and its number that day, from its name:
    /// `nightly-2026-11-16.2` is `("2026-11-16", 2)`, the day's first is 1.
    fn nightly_day(version: &str) -> Option<(&str, u64)> {
        let rest = version.trim().strip_prefix("nightly-")?;
        let (day, n) = match rest.split_once('.') {
            Some((day, n)) => (day, n.parse().ok()?),
            None => (rest, 1),
        };
        (day.len() == 10).then_some((day, n))
    }

    /// Where the list's version stands, for an app built on `built` (its
    /// commit's day) running `here`. The list's `build` is not signed, the
    /// version is, by the file's signature: the build counts only for a
    /// nightly dated no earlier than this app and after its own nightly, so
    /// an old signed file listed with a high number is never offered.
    pub fn listed_rank(latest: &Latest, built: &str, here: &str) -> Option<u64> {
        match nightly_day(&latest.version) {
            Some(day) if day.0 >= built && nightly_day(here).is_none_or(|h| day > h) => {
                rank(&latest.version, latest.build)
            }
            Some(_) => None,
            None => rank(&latest.version, None),
        }
    }

    fn listed(latest: &Latest) -> Option<u64> {
        listed_rank(latest, app_brand::BUILD_DATE, app_brand::VERSION)
    }

    /// The list's version is newer than `current`.
    pub fn newer(latest: &Latest, current: Option<u64>) -> bool {
        matches!((listed(latest), current), (Some(l), Some(c)) if l > c)
    }

    /// The list's version is older than `current`.
    pub fn older(latest: &Latest, current: Option<u64>) -> bool {
        matches!((listed(latest), current), (Some(l), Some(c)) if l < c)
    }

    /// A version that reads this build's database: the same schema steps.
    /// An older app refuses a database a newer schema wrote.
    pub fn same_schema(schema: Option<usize>) -> bool {
        schema == Some(app_core::db::schema_version())
    }

    /// The list's version is offered: newer, or, asked for, older with the
    /// same schema. The same version never is.
    pub fn offered(latest: &Latest, current: Option<u64>, allow_older: bool) -> bool {
        newer(latest, current)
            || (allow_older && older(latest, current) && same_schema(latest.schema))
    }

    /// `latest.json`'s key for this device: `windows-x86_64`,
    /// `linux-x86_64`, `android-aarch64`, `android-arm`, `android-x86_64`
    /// (the last three the same APK), and `linux-x86_64-pacman` for the Arch
    /// package.
    pub fn platform_key() -> String {
        key_for(super::pacman())
    }

    pub fn key_for(pacman: bool) -> String {
        let key = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
        if pacman { format!("{key}-pacman") } else { key }
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

    /// Downloads `url` into `part`, going on from what an earlier try left
    /// there when the server allows it, refused past `max` bytes. A slow
    /// connection takes as long as it needs, one where nothing arrives for a
    /// minute gives up. `progress` hears the bytes so far and the size.
    pub fn fetch(
        url: &str,
        part: &Path,
        max: u64,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> CommandResult<()> {
        let offline = |e: &dyn std::fmt::Display| failed("offline", e);
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(30)))
            .timeout_recv_response(Some(Duration::from_secs(60)))
            .timeout_per_read(Some(Duration::from_secs(60)))
            .user_agent(app_brand::SLUG)
            .build()
            .into();
        let have = fs::metadata(part).map_or(0, |m| m.len());
        let mut request = agent.get(url);
        if have > 0 {
            request = request.header("Range", format!("bytes={have}-"));
        }
        let response = match request.call() {
            Ok(response) => response,
            // what was kept is no longer a start of the file: from the beginning
            Err(ureq::Error::StatusCode(416)) => {
                fs::remove_file(part).map_err(|e| failed("file", e))?;
                return fetch(url, part, max, progress);
            }
            Err(e) => return Err(offline(&e)),
        };
        // 206 goes on from `have`, anything else is the whole file again
        let start = if response.status() == 206 { have } else { 0 };
        let total = response
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .map(|length| length + start);
        if total.is_some_and(|t| t > max) {
            return Err(failed("update", "the download is larger than it may be"));
        }
        let mut out = OpenOptions::new()
            .create(true)
            .write(true)
            .append(start > 0)
            .truncate(start == 0)
            .open(part)
            .map_err(|e| failed("file", e))?;
        let mut reader = response.into_body().into_reader();
        let mut buffer = vec![0; 64 * 1024];
        let mut done = start;
        progress(done, total);
        loop {
            let read = reader.read(&mut buffer).map_err(|e| offline(&e))?;
            if read == 0 {
                break;
            }
            out.write_all(&buffer[..read])
                .map_err(|e| failed("file", e))?;
            done += read as u64;
            if done > max {
                drop(out);
                let _ = fs::remove_file(part);
                return Err(failed("update", "the download is larger than it may be"));
            }
            progress(done, total);
        }
        out.flush().map_err(|e| failed("file", e))?;
        if total.is_some_and(|t| t != done) {
            return Err(failed("offline", "the download stopped before its end"));
        }
        Ok(())
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
            // nor the day's first nightly read out of its fix's name, `.2`
            let fix = after
                .strip_prefix('.')
                .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()));
            matches!(before, Some('_' | '-'))
                && !fix
                && (after.starts_with(['_', '.'])
                    || (after.starts_with('-') && !after.starts_with("-beta.")))
        })
    }

    /// This device's file from the list, downloaded into `part` and
    /// checked: signed by the update key, for the version the list
    /// announces. A file that fails the check is not kept to go on from.
    pub fn download(
        file: &Platform,
        version: &str,
        part: &Path,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> CommandResult<Vec<u8>> {
        fetch(&file.url, part, MAX_FILE, progress)?;
        let bytes = fs::read(part).map_err(|e| failed("file", e))?;
        let checked = verify(&bytes, &file.signature, app_brand::APP_UPDATES_KEY)
            .map_err(|e| failed("update", e))
            .and_then(|comment| {
                if signed_for(&comment, version) {
                    Ok(())
                } else {
                    Err(failed("update", "the file is signed for another version"))
                }
            });
        if let Err(e) = checked {
            let _ = fs::remove_file(part);
            return Err(e);
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

    use super::list::{Latest, clean, download, here, latest, offered, platform_key};
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
        if !offered(&list, here(), allow_older) {
            return Ok(None);
        }
        let Some(file) = list.platforms.get(&platform_key()) else {
            return Ok(None);
        };
        let dir = app
            .path()
            .app_cache_dir()
            .map_err(|e| failed("file", e))?
            .join("updates");
        std::fs::create_dir_all(&dir).map_err(|e| failed("file", e))?;
        let part = dir.join(format!("{}-{}.part", app_brand::SLUG, clean(&list.version)));
        // only this version's download is gone on from
        for entry in std::fs::read_dir(&dir)
            .map_err(|e| failed("file", e))?
            .flatten()
        {
            if entry.path() != part {
                let _ = std::fs::remove_file(entry.path());
            }
        }
        let bytes = download(file, &list.version, &part, &mut super::reporter(app))?;
        let _ = std::fs::remove_file(&part);
        let ready = Ready {
            channel,
            info: AppUpdate::new(&list, None),
            bytes,
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
        let replaced = replace(app, &ready);
        // nothing changed: the same update can be tried again
        if replaced.is_err() {
            *app.state::<Pending>()
                .0
                .lock()
                .map_err(|e| failed("unknown", e))? = Some(ready);
        }
        replaced
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
    /// first, then renamed over it, which the running app does not mind. The
    /// Arch package is installed by pacman instead, and the app keeps running
    /// until the person restarts it.
    #[cfg(target_os = "linux")]
    fn replace(app: &AppHandle, ready: &Ready) -> CommandResult<()> {
        use std::path::PathBuf;

        if super::pacman() {
            return super::pacman::install(&ready.bytes, &ready.info.version);
        }
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
            .map_or_default(|n| n.to_string_lossy().into_owned());
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

#[cfg(target_os = "linux")]
pub(crate) mod pacman {
    //! Installing the Arch package's update: the checked file written to a
    //! folder of its own that only this user reads, handed to
    //! `pkexec pacman -U`, then removed whether it installed or not.

    use std::ffi::OsStr;
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
    use std::path::Path;
    use std::process::{Command, Stdio};

    use super::failed;
    use crate::error::CommandResult;

    const PACMAN: &str = "/usr/bin/pacman";
    const PKEXEC: &str = "/usr/bin/pkexec";

    /// pkexec's own exit codes: the password window was closed, and it was
    /// refused or could not ask (no polkit agent).
    const CANCELLED: i32 = 126;
    const NOT_AUTHORIZED: i32 = 127;

    /// What runs pacman as root: pkexec, or nothing when the app already is.
    fn elevation() -> Vec<&'static OsStr> {
        let root = fs::metadata("/proc/self").is_ok_and(|m| m.uid() == 0);
        if root {
            vec![OsStr::new(PACMAN)]
        } else {
            vec![OsStr::new(PKEXEC), OsStr::new(PACMAN)]
        }
    }

    /// Installs `bytes`, the package of `version`, already checked against
    /// its signature.
    pub fn install(bytes: &[u8], version: &str) -> CommandResult<()> {
        install_with(&elevation(), &std::env::temp_dir(), bytes, version)
    }

    /// [`install`] with `program` (pacman and what runs it as root) and the
    /// folder the file is written in, for the tests.
    pub(crate) fn install_with(
        program: &[&OsStr],
        temp: &Path,
        bytes: &[u8],
        version: &str,
    ) -> CommandResult<()> {
        // a package is a zstd archive
        if !bytes.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
            return Err(failed("update", "the download is not an Arch package"));
        }
        let dir = temp.join(format!(
            "{}-update-{}-{}",
            app_brand::SLUG,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        // made here, never one that was waiting under this name
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&dir)
            .map_err(|e| failed("file", e))?;
        let installed = write_and_run(program, &dir, bytes, version);
        let _ = fs::remove_dir_all(&dir);
        installed
    }

    fn write_and_run(
        program: &[&OsStr],
        dir: &Path,
        bytes: &[u8],
        version: &str,
    ) -> CommandResult<()> {
        let file = dir.join(format!(
            "{}_{}_x86_64.pkg.tar.zst",
            app_brand::DISPLAY_NAME.replace(' ', "-"),
            super::list::clean(version)
        ));
        let mut out = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&file)
            .map_err(|e| failed("file", e))?;
        out.write_all(bytes)
            .and_then(|()| out.sync_all())
            .map_err(|e| failed("file", e))?;
        drop(out);
        // read back right before it installs: the file checked is the one
        // pacman reads
        if fs::read(&file).map_err(|e| failed("file", e))? != bytes {
            return Err(failed("update", "the package changed after it was checked"));
        }
        let Some((first, rest)) = program.split_first() else {
            return Err(failed("unknown", "no program to install with"));
        };
        let output = Command::new(first)
            .args(rest)
            .args([OsStr::new("-U"), OsStr::new("--noconfirm")])
            .arg(&file)
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .output();
        let output = match output {
            Ok(output) => output,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(failed(
                    "polkitMissing",
                    format!("{} is missing: {e}", first.to_string_lossy()),
                ));
            }
            Err(e) => return Err(failed("pacman", e)),
        };
        if output.status.success() {
            return Ok(());
        }
        let said = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
        let stderr = said(&output.stderr);
        let elevated = program.len() > 1;
        match output.status.code() {
            Some(CANCELLED) if elevated => Err(failed(
                "passwordCancelled",
                "the password window was closed",
            )),
            Some(NOT_AUTHORIZED) if elevated => {
                let line = last_lines(&stderr);
                // no agent to show the window, or no polkit running to ask
                if stderr.contains("authentication agent") || stderr.contains("authority") {
                    Err(failed("polkitMissing", line))
                } else {
                    Err(failed("notAllowed", line))
                }
            }
            _ => Err(failed(
                "pacman",
                last_lines(&format!("{}\n{stderr}", said(&output.stdout))),
            )),
        }
    }

    /// What a failed run said last: pacman's `error:` lines, the last two,
    /// else its last two lines.
    pub(crate) fn last_lines(text: &str) -> String {
        let lines: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        let errors: Vec<&str> = lines
            .iter()
            .copied()
            .filter(|l| l.starts_with("error:"))
            .collect();
        let picked = if errors.is_empty() { &lines } else { &errors };
        picked[picked.len().saturating_sub(2)..].join(" ")
    }
}

#[cfg(target_os = "android")]
mod platform {
    //! Downloaded and checked here, installed by the system installer.

    use std::fs;
    use std::path::PathBuf;

    use tauri::{AppHandle, Manager};

    use super::list::{clean, download, here, latest, offered, platform_key};
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
        let current = offered(&latest, here(), allow_older);
        let name = format!("{}-{}.apk", app_brand::SLUG, clean(&latest.version));
        let part_name = format!("{name}.part");
        // only the newest file stays, with its download in progress; an
        // installed or older one goes
        for entry in fs::read_dir(&dir).map_err(|e| failed("file", e))?.flatten() {
            let file = entry.file_name().to_string_lossy().into_owned();
            if !current || (file != name && file != part_name) {
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
        // downloaded under another name: a file named like an update has
        // always been checked
        let part = dir.join(&part_name);
        download(file, &latest.version, &part, &mut super::reporter(app))?;
        fs::rename(&part, dir.join(&name)).map_err(|e| failed("file", e))?;
        Ok(Some(found))
    }
}

#[cfg(desktop)]
mod page {
    //! The portable exe and a Linux package: `latest.json`
    //! read for its version only, nothing downloaded, the version's release
    //! page has the file.

    use std::sync::Mutex;

    use super::list::{here, latest, newer};
    use super::{AppUpdate, Channel, failed, release_page, releases_in};
    use crate::error::CommandResult;

    /// The version the last look found, whose page opens.
    static FOUND: Mutex<Option<String>> = Mutex::new(None);

    pub fn check(channel: Channel) -> CommandResult<Option<AppUpdate>> {
        let latest = latest(channel)?;
        let found = newer(&latest, here())
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
        crate::sync::browser::open_in_browser(&page).map_err(|e| failed("browser", e))
    }
}

/// This build only points to the release page, see [`page_only`]: the page
/// says so instead of how updates download.
#[tauri::command]
#[must_use]
pub fn app_update_page_only() -> bool {
    page_only()
}

/// This build is the Arch package, see [`pacman`]: it installs without
/// restarting.
#[tauri::command]
#[must_use]
pub fn app_update_pacman() -> bool {
    pacman()
}

/// Starts the app again, into the version the Arch package installed.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "tauri hands commands the handle by value"
)]
pub fn app_update_restart(app: tauri::AppHandle) {
    app.restart()
}

/// Looks for a newer version and downloads it, when the person asks for
/// it: the app never does on its own. `None`: up to date, or this build
/// does not update itself. `allow_older`: a beta going back to Stable,
/// an older version with the same schema downloads too (never on the
/// portable exe, which only reads the version).
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

/// The newest version on the release list, downloading nothing: how the app
/// looks on its own, to tell of a new version or a critical security fix;
/// the person chooses whether to update. `allow_older`: an older
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
        let shown = list::newer(&latest, list::here())
            || (allow_older && list::older(&latest, list::here()));
        Ok(shown
            .then(|| AppUpdate::new(&latest, page_only().then(|| release_page(&latest.version)))))
    })
    .await
    .map_err(|e| failed("unknown", e))?
}

/// Desktop: installs the downloaded update and starts the app again. The
/// portable exe and a Linux package open the release page instead.
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
    #[test]
    fn a_cut_download_goes_on_from_where_it_stopped() {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;

        let file: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/app", listener.local_addr().unwrap());
        let served = file.clone();
        let host = std::thread::spawn(move || {
            let mut ranges = Vec::new();
            for (n, stream) in listener.incoming().take(2).enumerate() {
                let mut stream = stream.unwrap();
                let mut range = None;
                let reader = BufReader::new(stream.try_clone().unwrap());
                for line in reader.lines() {
                    let line = line.unwrap();
                    if line.is_empty() {
                        break;
                    }
                    if let Some(r) = line.to_lowercase().strip_prefix("range: bytes=") {
                        range = r.trim_end_matches('-').parse::<usize>().ok();
                    }
                }
                ranges.push(range);
                if n == 0 {
                    // the whole file announced, the connection cut at 400
                    write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\n").unwrap();
                    stream.write_all(&served[..400]).unwrap();
                } else {
                    let from = range.unwrap_or(0);
                    write!(
                        stream,
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\n\r\n",
                        served.len() - from
                    )
                    .unwrap();
                    stream.write_all(&served[from..]).unwrap();
                }
            }
            ranges
        });
        let dir = std::env::temp_dir().join(format!("update-resume-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let part = dir.join("app.part");
        let mut seen = Vec::new();
        let first = super::list::fetch(&url, &part, 10_000, &mut |done, total| {
            seen.push((done, total));
        });
        assert!(
            first.is_err(),
            "a cut download is not taken for a whole one"
        );
        assert_eq!(std::fs::metadata(&part).unwrap().len(), 400);
        super::list::fetch(&url, &part, 10_000, &mut |done, total| {
            seen.push((done, total));
        })
        .unwrap();
        assert_eq!(std::fs::read(&part).unwrap(), file);
        assert_eq!(seen.last(), Some(&(1000, Some(1000))));
        assert_eq!(host.join().unwrap(), [None, Some(400)]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    use super::list::{
        Latest, clean, listed_rank, newer, offered, older, rank, same_schema, signed_for, verify,
    };
    use super::{Channel, release_page, releases_in};

    fn list(version: &str, schema: Option<usize>) -> Latest {
        Latest {
            version: version.to_owned(),
            notes: String::new(),
            messages: Vec::new(),
            changelog: String::new(),
            critical: false,
            platforms: std::collections::HashMap::new(),
            schema,
            build: None,
        }
    }

    #[test]
    fn reads_the_messages_and_changelog_and_does_without_them() {
        let with: Latest = serde_json::from_str(
            r#"{"version":"0.5.5","messages":["faster","platform:Linux"],"changelog":"- Long notes open faster.","platforms":{}}"#,
        )
        .unwrap();
        assert_eq!(with.messages, ["faster", "platform:Linux"]);
        assert_eq!(with.changelog, "- Long notes open faster.");
        let without: Latest =
            serde_json::from_str(r#"{"version":"0.5.4","notes":"Fixes.","platforms":{}}"#).unwrap();
        assert!(without.messages.is_empty() && without.changelog.is_empty());
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
        let beta = rank("0.5.0-beta.2", None);
        assert!(older(&list("0.4.1", None), beta));
        assert!(!older(&list("0.5.0", None), beta));
        assert!(!older(&list("0.4.1", None), rank("0.4.1", None)));
        assert!(!older(&list("latest", None), rank("0.4.1", None)));
        // newer is always offered, asked or not
        assert!(offered(&list("0.5.0", None), beta, false));
        assert!(offered(&list("0.5.0", None), beta, true));
        // older only when asked, and only with the same schema
        assert!(!offered(&list("0.4.1", here), beta, false));
        assert!(offered(&list("0.4.1", here), beta, true));
        assert!(!offered(&list("0.4.1", Some(1)), beta, true));
        assert!(!offered(&list("0.4.1", None), beta, true));
        // the same version never
        assert!(!offered(&list("0.4.1", here), rank("0.4.1", None), true));
    }

    #[test]
    fn the_channel_comes_from_the_ui() {
        let read = |text: &str| serde_json::from_str::<Channel>(text).ok();
        assert_eq!(read(r#""stable""#), Some(Channel::Stable));
        assert_eq!(read(r#""nightly""#), Some(Channel::Nightly));
        // betas ended: someone who followed them follows Stable
        assert_eq!(read(r#""beta""#), Some(Channel::Stable));
        assert_eq!(read(r#""weekly""#), None);
        assert_eq!(Channel::default(), Channel::Stable);
    }

    #[test]
    fn each_channel_has_its_list() {
        assert_eq!(Channel::Stable.url(), app_brand::APP_UPDATES_URL);
        assert_eq!(Channel::Nightly.url(), app_brand::APP_NIGHTLY_UPDATES_URL);
        assert_ne!(Channel::Stable.url(), Channel::Nightly.url());
        // both lists live in the same repository, whose pages the app opens
        assert_eq!(
            releases_in(Channel::Stable.url()),
            releases_in(Channel::Nightly.url())
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
        let newer = |a: &str, b: &str| newer(&list(a, None), rank(b, None));
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

    #[test]
    fn nightlies_sit_between_two_releases() {
        // the build numbers tools/version.mjs gives: 0.6.0 is 600099
        assert_eq!(rank("0.6.0", None), Some(600_099));
        assert_eq!(rank("0.6.1", None), Some(600_199));
        assert_eq!(
            rank("nightly-2026-11-16", None),
            None,
            "a name alone says nothing"
        );
        let first = Some(600_100);
        let fix = Some(600_101);
        let nightly = |name: &str, build| Latest {
            build,
            ..list(name, None)
        };
        // the day's fix after the day's first, the next release after both
        // (named far ahead, so the app running the test is older)
        assert!(newer(&nightly("nightly-2999-11-16.2", fix), first));
        assert!(!newer(&nightly("nightly-2999-11-16", first), fix));
        assert!(newer(&list("0.6.1", None), fix));
        // a nightly is newer than the release before it, older than the next
        assert!(newer(
            &nightly("nightly-2999-11-16", first),
            rank("0.6.0", None)
        ));
        assert!(older(&list("0.6.0", None), first));
    }

    #[test]
    fn a_build_number_only_counts_for_a_newer_nightly() {
        let high = |name: &str| Latest {
            build: Some(99_999_999),
            ..list(name, None)
        };
        let ranked = |name: &str, built, here| listed_rank(&high(name), built, here);
        // an old release listed with a high number keeps its own
        assert_eq!(ranked("0.3.0", "2026-11-16", "0.6.2"), rank("0.3.0", None));
        // a nightly older than this app, or than its own nightly, is not ranked
        assert_eq!(ranked("nightly-2026-11-10", "2026-11-16", "0.6.2"), None);
        assert_eq!(
            ranked("nightly-2026-11-16", "2026-11-16", "nightly-2026-11-16.2"),
            None
        );
        assert_eq!(
            ranked("nightly-2026-11-16", "2026-11-16", "nightly-2026-11-16"),
            None
        );
        // the next nightly, and the day's fix, are
        assert_eq!(
            ranked("nightly-2026-11-17", "2026-11-16", "nightly-2026-11-16"),
            Some(99_999_999)
        );
        assert_eq!(
            ranked("nightly-2026-11-16.2", "2026-11-16", "nightly-2026-11-16"),
            Some(99_999_999)
        );
        assert_eq!(
            ranked("nightly-2026-11-16", "2026-11-16", "0.6.2"),
            Some(99_999_999)
        );
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
        // a nightly by its name, never the day's first read out of its fix
        let fix = "timestamp:1\tfile:ficelle-nightly-2026-11-16.2-android.apk";
        assert!(signed_for(fix, "nightly-2026-11-16.2"));
        assert!(
            !signed_for(fix, "nightly-2026-11-16"),
            "a fix passed off as the first"
        );
        assert!(signed_for(
            "timestamp:1\tfile:Ficelle_nightly-2026-11-16_amd64.AppImage",
            "nightly-2026-11-16"
        ));
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
        let part = std::env::temp_dir().join(format!("update-check-{}.part", std::process::id()));
        let bytes = super::list::download(file, &latest.version, &part, &mut |_, _| {}).unwrap();
        let _ = std::fs::remove_file(&part);
        assert!(bytes.starts_with(b"MZ"), "a Windows program");
    }

    #[test]
    fn the_arch_package_is_its_program_with_its_mark() {
        use super::installed_by_pacman;

        let root = std::env::temp_dir().join(format!("update-pacman-{}", std::process::id()));
        let exe = root.join("usr/bin").join(app_brand::SLUG);
        let mark = root
            .join("usr/lib")
            .join(app_brand::SLUG)
            .join("arch-package");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, "").unwrap();
        // the old AUR package, or a copy of the program, has no mark
        assert!(!installed_by_pacman(&exe, &root));
        std::fs::create_dir_all(mark.parent().unwrap()).unwrap();
        std::fs::write(&mark, "").unwrap();
        assert!(installed_by_pacman(&exe, &root));
        // an AppImage runs its program from where it is mounted
        let mounted = root
            .join("tmp/.mount_FicelleX/usr/bin")
            .join(app_brand::SLUG);
        assert!(!installed_by_pacman(&mounted, &root));
        assert!(!installed_by_pacman(
            &root.join("opt").join(app_brand::SLUG),
            &root
        ));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_arch_package_has_its_own_file_in_the_list() {
        use super::list::key_for;

        let latest: Latest = serde_json::from_str(
            r#"{"version":"0.9.0","platforms":{
            "linux-x86_64":{"signature":"a","url":"https://example.org/Ficelle_0.9.0_amd64.AppImage"},
            "linux-x86_64-pacman":{"signature":"p","url":"https://example.org/Ficelle_0.9.0_x86_64.pkg.tar.zst"}}}"#,
        )
        .unwrap();
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            assert_eq!(key_for(false), "linux-x86_64");
            assert_eq!(key_for(true), "linux-x86_64-pacman");
            assert_eq!(latest.platforms[&key_for(true)].signature, "p");
            // the AppImage keeps its own key, which older apps read
            assert_eq!(latest.platforms[&key_for(false)].signature, "a");
        }
        assert!(key_for(true).ends_with("-pacman"));
        // a list from before the package has no file for it: nothing offered
        let before: Latest = serde_json::from_str(
            r#"{"version":"0.8.6","platforms":{"linux-x86_64":{"signature":"a","url":"u"}}}"#,
        )
        .unwrap();
        assert!(!before.platforms.contains_key(&key_for(true)));
        // the package's file names its version for the signature's check
        assert!(signed_for(
            "timestamp:1\tfile:Ficelle_0.9.0_x86_64.pkg.tar.zst",
            "0.9.0"
        ));
        assert!(!signed_for(
            "timestamp:1\tfile:Ficelle_0.9.0_x86_64.pkg.tar.zst",
            "0.9.1"
        ));
    }

    #[cfg(target_os = "linux")]
    mod pacman {
        use std::ffi::OsStr;
        use std::path::{Path, PathBuf};

        use super::super::pacman::{install_with, last_lines};

        /// A zstd archive's start, all `install_with` looks at.
        const PACKAGE: &[u8] = &[0x28, 0xb5, 0x2f, 0xfd, 1, 2, 3];

        fn temp(name: &str) -> PathBuf {
            let dir = std::env::temp_dir().join(format!("update-{name}-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            dir
        }

        /// What is left in `dir` after a try: nothing, the file is removed.
        fn left(dir: &Path) -> usize {
            std::fs::read_dir(dir).unwrap().count()
        }

        /// pkexec stood in for by a shell running `script`, with pacman's
        /// arguments after it.
        fn run(dir: &Path, script: &str) -> Result<(), (String, String)> {
            let program = ["sh", "-c", script, "pkexec", "/usr/bin/pacman"].map(OsStr::new);
            install_with(&program, dir, PACKAGE, "0.9.0")
                .map_err(|e| (e.kind().to_owned(), e.message().to_owned()))
        }

        #[test]
        fn installs_the_file_it_was_given_then_removes_it() {
            let dir = temp("pacman-ok");
            // pacman gets -U, --noconfirm and the file, still there
            run(
                &dir,
                r#"[ "$1 $2 $3" = "/usr/bin/pacman -U --noconfirm" ] && [ -f "$4" ] && case "$4" in *_0.9.0_x86_64.pkg.tar.zst) ;; *) exit 9 ;; esac"#,
            )
            .unwrap();
            assert_eq!(left(&dir), 0);
            std::fs::remove_dir_all(&dir).unwrap();
        }

        #[test]
        fn the_file_is_only_this_users() {
            let dir = temp("pacman-mode");
            let modes = dir.join("modes");
            let script = format!(r#"stat -c %a "$(dirname "$4")" "$4" > {}"#, modes.display());
            run(&dir, &script).unwrap();
            assert_eq!(std::fs::read_to_string(&modes).unwrap(), "700\n600\n");
            std::fs::remove_dir_all(&dir).unwrap();
        }

        #[test]
        fn a_closed_password_window_changes_nothing() {
            let dir = temp("pacman-cancel");
            assert_eq!(run(&dir, "exit 126").unwrap_err().0, "passwordCancelled");
            assert_eq!(left(&dir), 0);
            std::fs::remove_dir_all(&dir).unwrap();
        }

        #[test]
        fn no_polkit_is_said_as_such() {
            let dir = temp("pacman-polkit");
            let agent = "echo 'Error executing command as another user: No authentication agent found.' >&2; exit 127";
            assert_eq!(run(&dir, agent).unwrap_err().0, "polkitMissing");
            let stopped = "echo 'Error getting authority: Error initializing authority: Could not connect: No such file or directory' >&2; exit 127";
            assert_eq!(run(&dir, stopped).unwrap_err().0, "polkitMissing");
            let refused =
                "echo 'Error executing command as another user: Not authorized' >&2; exit 127";
            assert_eq!(run(&dir, refused).unwrap_err().0, "notAllowed");
            let missing = [
                OsStr::new("/nonexistent/pkexec"),
                OsStr::new("/usr/bin/pacman"),
            ];
            let (kind, _) = install_with(&missing, &dir, PACKAGE, "0.9.0")
                .map_err(|e| (e.kind(), e.message().to_owned()))
                .unwrap_err();
            assert_eq!(kind, "polkitMissing");
            assert_eq!(left(&dir), 0);
            std::fs::remove_dir_all(&dir).unwrap();
        }

        #[test]
        fn a_failed_pacman_says_its_last_errors() {
            let dir = temp("pacman-fail");
            let script = "echo 'loading packages...'; echo 'error: failed to commit transaction (conflicting files)' >&2; \
                echo 'ficelle: /usr/bin/ficelle exists in filesystem' >&2; echo 'Errors occurred, no packages were upgraded.' >&2; exit 1";
            let (kind, message) = run(&dir, script).unwrap_err();
            assert_eq!(kind, "pacman");
            assert_eq!(
                message,
                "error: failed to commit transaction (conflicting files)"
            );
            assert_eq!(left(&dir), 0);
            std::fs::remove_dir_all(&dir).unwrap();
        }

        #[test]
        fn only_a_package_is_handed_to_pacman() {
            let dir = temp("pacman-elf");
            let program = ["sh", "-c", "exit 0", "pkexec"].map(OsStr::new);
            let err = install_with(&program, &dir, b"\x7fELF", "0.9.0").unwrap_err();
            assert_eq!(err.kind(), "update");
            assert_eq!(left(&dir), 0);
            std::fs::remove_dir_all(&dir).unwrap();
        }

        #[test]
        fn the_last_lines_prefer_pacmans_errors() {
            assert_eq!(
                last_lines("a\nerror: one\nb\nerror: two\nerror: three\nc\n"),
                "error: two error: three"
            );
            assert_eq!(last_lines("a\n\nb\nc\n"), "b c");
            assert_eq!(last_lines(""), "");
        }

        /// The real thing, as root in an Arch container with a package of
        /// this app installed: the newer package in `UPDATE_PACKAGE`, its
        /// signature (tauri's .sig) beside it, signed by the key in
        /// `UPDATE_KEY` (tauri's .pub), for `UPDATE_VERSION`. Checked and
        /// installed the way the app does, pacman run straight as root.
        #[test]
        #[ignore = "installs a package with pacman, as root"]
        fn pacman_updates_the_installed_package() {
            use super::super::list::{signed_for, verify};

            let var = |name| std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set"));
            let file = PathBuf::from(var("UPDATE_PACKAGE"));
            let version = var("UPDATE_VERSION");
            let bytes = std::fs::read(&file).unwrap();
            let signature = std::fs::read_to_string(format!("{}.sig", file.display())).unwrap();
            let key = std::fs::read_to_string(var("UPDATE_KEY")).unwrap();
            let comment = verify(&bytes, &signature, &key).unwrap();
            assert!(signed_for(&comment, &version), "{comment}");
            super::super::pacman::install(&bytes, &version).unwrap();
            // the package is the release's, whatever build runs the test
            let query = std::process::Command::new("pacman")
                .args(["-Q", "ficelle"])
                .output()
                .unwrap();
            let installed = String::from_utf8_lossy(&query.stdout);
            assert_eq!(installed.trim(), format!("ficelle {version}-1"));
        }
    }

    #[test]
    fn another_key_fails() {
        let other = app_brand::APP_UPDATES_KEY;
        assert!(verify(FILE, SIGNATURE, other).is_err());
        assert!(verify(FILE, SIGNATURE, "not base64").is_err());
        assert!(verify(FILE, "", KEY).is_err());
    }
}
