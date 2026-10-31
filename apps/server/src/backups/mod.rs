// SPDX-License-Identifier: AGPL-3.0-or-later

//! The instance's backups, set up by an admin: where (a folder on the
//! server, S3, `WebDAV`, SFTP), the password, when, what to keep. Runs on a
//! schedule in the background, the admin page shows how they went, a
//! failure is logged and mailed to the admins. People back up their own
//! notes from their apps.

pub mod instance;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use app_repo::{
    FolderTarget, KdfParams, Policy, Repo, S3Config, S3Target, Target, WebdavConfig, WebdavTarget,
};
use base64::Engine;
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::db::DbError;
use crate::keys::MasterKey;
use crate::routes::AppState;
use crate::sftp::{SftpConfig, SftpTarget};
use instance::RunReport;

/// Where the instance's backups go.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ServerTarget {
    /// A folder the server can write to: another disk, a mounted share.
    Folder {
        /// Its path on the server.
        path: String,
    },
    /// An S3 bucket.
    S3(S3Config),
    /// A `WebDAV` folder.
    Webdav(WebdavConfig),
    /// An SFTP folder.
    Sftp(SftpConfig),
}

/// A folder the admin page may back up to: an absolute path that is not
/// there yet, empty, or a repository already (its `config` file). A folder
/// holding anything else, like the server's own files or a system folder,
/// is never written into or pruned.
fn folder_allowed(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("the folder must be a full path, like /backups".into());
    }
    let Ok(mut entries) = std::fs::read_dir(path) else {
        return if path.exists() {
            Err("this is not a folder the server can read".into())
        } else {
            Ok(())
        };
    };
    if entries.next().is_none() || path.join("config").is_file() {
        Ok(())
    } else {
        Err("this folder holds other files: choose an empty folder or a backup".into())
    }
}

impl ServerTarget {
    /// The target to use.
    ///
    /// # Errors
    ///
    /// An incomplete config, in words.
    pub fn open(&self) -> Result<Arc<dyn Target>, String> {
        Ok(match self {
            Self::Folder { path } => {
                if path.trim().is_empty() {
                    return Err("a folder is needed".into());
                }
                folder_allowed(Path::new(path.trim()))?;
                Arc::new(FolderTarget::new(path.trim()))
            }
            Self::S3(c) => Arc::new(S3Target::new(c.clone()).map_err(|e| e.to_string())?),
            Self::Webdav(c) => Arc::new(WebdavTarget::new(c).map_err(|e| e.to_string())?),
            Self::Sftp(c) => Arc::new(SftpTarget::new(c.clone()).map_err(|e| e.to_string())?),
        })
    }

    /// The same without its secrets, to show.
    #[must_use]
    pub fn without_secrets(&self) -> Self {
        match self {
            Self::Folder { path } => Self::Folder { path: path.clone() },
            Self::S3(c) => Self::S3(S3Config {
                secret_key: String::new(),
                ..c.clone()
            }),
            Self::Webdav(c) => Self::Webdav(WebdavConfig {
                password: String::new(),
                ..c.clone()
            }),
            Self::Sftp(c) => Self::Sftp(SftpConfig {
                password: String::new(),
                private_key: String::new(),
                passphrase: String::new(),
                ..c.clone()
            }),
        }
    }

    /// The secrets of `old` where these are blank: the page sends them
    /// back blank when unchanged. Only for the same place (server, account,
    /// bucket): someone pointing the backups elsewhere types them again,
    /// else a stolen admin session could have the server hand its stored
    /// password or key to a server of theirs.
    #[must_use]
    pub fn keeping_secrets_of(self, old: &Self) -> Self {
        let keep = |new: &mut String, old: &String| {
            if new.is_empty() {
                new.clone_from(old);
            }
        };
        if !self.same_place(old) {
            return self;
        }
        match (self, old) {
            (Self::S3(mut new), Self::S3(old)) => {
                keep(&mut new.secret_key, &old.secret_key);
                Self::S3(new)
            }
            (Self::Webdav(mut new), Self::Webdav(old)) => {
                keep(&mut new.password, &old.password);
                Self::Webdav(new)
            }
            (Self::Sftp(mut new), Self::Sftp(old)) => {
                keep(&mut new.password, &old.password);
                keep(&mut new.private_key, &old.private_key);
                keep(&mut new.passphrase, &old.passphrase);
                Self::Sftp(new)
            }
            (new, _) => new,
        }
    }

    /// The same server and account: a different folder is still the same
    /// place, a different host or user is not.
    fn same_place(&self, old: &Self) -> bool {
        let origin = |address: &str| {
            url::Url::parse(address).ok().map(|u| {
                (
                    u.scheme().to_owned(),
                    u.host_str().map(str::to_ascii_lowercase),
                    u.port_or_known_default(),
                )
            })
        };
        match (self, old) {
            (Self::S3(new), Self::S3(old)) => {
                new.endpoint == old.endpoint
                    && new.region == old.region
                    && new.bucket == old.bucket
                    && new.access_key == old.access_key
            }
            (Self::Webdav(new), Self::Webdav(old)) => {
                origin(&new.url).is_some()
                    && origin(&new.url) == origin(&old.url)
                    && new.username == old.username
            }
            (Self::Sftp(new), Self::Sftp(old)) => {
                // another host key may be another machine at that name:
                // the stored password or key is not sent to it
                new.host.eq_ignore_ascii_case(&old.host)
                    && new.port == old.port
                    && new.username == old.username
                    && new.host_key.trim() == old.host_key.trim()
            }
            _ => false,
        }
    }
}

/// How often.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Schedule {
    /// Only when asked.
    Off,
    /// Every hour.
    Hourly,
    /// Every day.
    #[default]
    Daily,
    /// Every week.
    Weekly,
}

impl Schedule {
    pub(crate) fn period_ms(self) -> Option<i64> {
        match self {
            Self::Off => None,
            Self::Hourly => Some(3_600_000),
            Self::Daily => Some(86_400_000),
            Self::Weekly => Some(7 * 86_400_000),
        }
    }
}

/// The instance's backup settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupConfig {
    /// Where.
    pub target: ServerTarget,
    /// The repository's password.
    pub password: String,
    /// When.
    pub schedule: Schedule,
    /// What to keep.
    pub policy: Policy,
    /// The time zone for the policy's days, seconds east of UTC.
    pub offset_seconds: i64,
}

/// How the runs went.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BackupStatus {
    /// The last run started.
    pub last_run_at: Option<i64>,
    /// The last that went through.
    pub last_success_at: Option<i64>,
    /// Why the last failed.
    pub last_error: Option<String>,
    /// What the last good run did.
    pub last_report: Option<RunReport>,
    /// Snapshots after the last run.
    pub snapshots: u64,
    /// Bytes at the target after the last run.
    pub stored_bytes: u64,
}

fn read_setting<T: serde::de::DeserializeOwned>(
    conn: &Connection,
    key: &str,
) -> Result<Option<T>, DbError> {
    let text: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(text.and_then(|t| serde_json::from_str(&t).ok()))
}

fn write_setting<T: Serialize>(
    conn: &Connection,
    key: &str,
    value: Option<&T>,
) -> Result<(), DbError> {
    match value {
        Some(value) => {
            let text = serde_json::to_string(value).unwrap_or_default();
            conn.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                [key, &text],
            )?;
        }
        None => {
            conn.execute("DELETE FROM settings WHERE key = ?1", [key])?;
        }
    }
    Ok(())
}

/// The settings, None when backups are not set up. They hold the
/// target's credentials and the repository password, so they are kept
/// sealed by the master key; settings from
/// before 0.3.0, kept plain, are sealed the first time they are read.
///
/// # Errors
///
/// Database errors.
pub fn config(conn: &Connection, master: &MasterKey) -> Result<Option<BackupConfig>, DbError> {
    let Some(stored) = read_setting::<serde_json::Value>(conn, "backups")? else {
        return Ok(None);
    };
    if let Some(sealed) = stored.get("sealed").and_then(|s| s.as_str()) {
        let opened = base64::engine::general_purpose::STANDARD
            .decode(sealed)
            .ok()
            .and_then(|bytes| master.open_setting("backups", &bytes))
            .and_then(|text| serde_json::from_slice(&text).ok());
        return Ok(opened);
    }
    let plain: Option<BackupConfig> = serde_json::from_value(stored).ok();
    if let Some(plain) = &plain {
        set_config(conn, master, Some(plain))?;
    }
    Ok(plain)
}

/// Sets or clears the settings, sealed.
///
/// # Errors
///
/// Database errors.
pub fn set_config(
    conn: &Connection,
    master: &MasterKey,
    config: Option<&BackupConfig>,
) -> Result<(), DbError> {
    let sealed = config.map(|config| {
        let text = zeroize::Zeroizing::new(serde_json::to_vec(config).unwrap_or_default());
        serde_json::json!({
            "sealed": base64::engine::general_purpose::STANDARD
                .encode(master.seal_setting("backups", &text)),
        })
    });
    write_setting(conn, "backups", sealed.as_ref())
}

/// How the runs went.
///
/// # Errors
///
/// Database errors.
pub fn status(conn: &Connection) -> Result<BackupStatus, DbError> {
    Ok(read_setting(conn, "backups_status")?.unwrap_or_default())
}

/// Records how the runs went.
///
/// # Errors
///
/// Database errors.
pub fn set_status(conn: &Connection, status: &BackupStatus) -> Result<(), DbError> {
    write_setting(conn, "backups_status", Some(status))
}

/// Opens or makes the repository a config names.
///
/// # Errors
///
/// The target refused, the wrong password, in words.
pub fn open(config: &BackupConfig, create: bool, now: i64) -> Result<Repo, String> {
    let target = config.target.open()?;
    if create {
        Repo::init(target, &config.password, KdfParams::DEFAULT, now)
    } else {
        Repo::open(target, &config.password)
    }
    .map_err(|e| match e {
        app_repo::Error::WrongPassword => "wrong password for this backup".into(),
        app_repo::Error::NotFound(_) => {
            "there is no backup at this place: it was moved or the place is not reachable".into()
        }
        other => other.to_string(),
    })
}

/// One run at a time on this server.
static RUNNING: AtomicBool = AtomicBool::new(false);

/// Whether a run is going on.
pub fn running() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

/// The master key's file, to back up with the database when it lives in
/// the data folder. A key kept apart (`storage.master_key_file`, a Docker
/// secret) stays apart: with it in the backups, a stolen backup would read
/// everything. Restoring then needs that
/// file.
fn master_key_path(state: &AppState) -> Option<PathBuf> {
    if !state.config.storage.master_key_file.as_os_str().is_empty() {
        return None;
    }
    let path = state.config.storage.data_dir.join(crate::keys::KEY_FILE);
    path.exists().then_some(path)
}

/// A scratch folder in the data folder, new.
pub fn scratch(state: &AppState, what: &str) -> PathBuf {
    state
        .config
        .storage
        .data_dir
        .join(format!("backup-{what}-{}", state.clock.now()))
}

/// Scratch folders a run, a check or a restore left when the server
/// stopped in the middle: they hold copies of the database, removed at
/// start. Nothing runs yet then.
pub fn remove_leftover_scratch(data_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(data_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with("backup-")
            && entry.file_type().is_ok_and(|t| t.is_dir())
        {
            remove_scratch(&entry.path());
        }
    }
}

fn remove_scratch(path: &Path) {
    if path.exists() {
        let _ = std::fs::remove_dir_all(path);
    }
}

/// Backs the instance up now, then thins the snapshots by the policy.
/// Records how it went, logs and mails a failure.
///
/// # Errors
///
/// Why it failed, in words (recorded too).
pub async fn run(state: &AppState, by: &str) -> Result<BackupStatus, String> {
    if RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("a backup is running already".into());
    }
    let result = run_once(state).await;
    RUNNING.store(false, Ordering::SeqCst);
    let now = state.clock.now();
    let failed = result.as_ref().err().cloned();
    let report = result.as_ref().ok().cloned();
    let by = by.to_owned();
    let status = state
        .db
        .call(move |c| {
            let mut status = status(c)?;
            status.last_run_at = Some(now);
            match (report, failed.as_ref()) {
                (Some((report, snapshots, stored)), _) => {
                    status.last_success_at = Some(now);
                    status.last_error = None;
                    status.last_report = Some(report);
                    status.snapshots = snapshots;
                    status.stored_bytes = stored;
                }
                (None, Some(why)) => status.last_error = Some(why.clone()),
                (None, None) => {}
            }
            set_status(c, &status)?;
            let (name, detail) = match &failed {
                Some(why) => ("backup_failed", format!("{by}: {why}")),
                None => ("backup_done", by.clone()),
            };
            crate::accounts::store::audit(
                c,
                now,
                &crate::accounts::store::Event {
                    name,
                    user_id: None,
                    device_id: None,
                    ip: None,
                    detail: Some(&detail),
                },
            )?;
            Ok(status)
        })
        .await
        .map_err(|e| e.to_string())?;
    if let Some(why) = &status.last_error.clone().filter(|_| result.is_err()) {
        alert(state, why).await;
        return Err(why.clone());
    }
    Ok(status)
}

async fn run_once(state: &AppState) -> Result<(RunReport, u64, u64), String> {
    let master = state.master.clone();
    let config = state
        .db
        .call(move |c| config(c, &master))
        .await
        .map_err(|e| e.to_string())?
        .ok_or("backups are not set up")?;
    let dir = scratch(state, "run");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let copy = dir.join("server.db");
    let copy_text = copy.to_string_lossy().into_owned();
    let copied = state
        .db
        .call(move |c| {
            c.execute("VACUUM INTO ?1", [&copy_text])?;
            Ok(())
        })
        .await
        .map_err(|e| format!("copying the database: {e}"));
    let feedback_text = dir
        .join(crate::db::FEEDBACK_FILE)
        .to_string_lossy()
        .into_owned();
    let copied = match copied {
        Ok(()) => state
            .feedback
            .call(move |c| {
                c.execute("VACUUM INTO ?1", [&feedback_text])?;
                Ok(())
            })
            .await
            .map_err(|e| format!("copying the feedback database: {e}")),
        failed => failed,
    };
    if let Err(e) = copied {
        remove_scratch(&dir);
        return Err(e);
    }
    let files = state.files.clone();
    let key = master_key_path(state);
    let clock = state.clock.clone();
    let result = tokio::task::spawn_blocking(move || {
        let now = || clock.now();
        let repo = open(&config, false, now())?;
        let report = instance::backup(&repo, &copy, key.as_deref(), files.as_ref(), &now)?;
        let snapshots: Vec<(String, i64)> = repo
            .snapshots()
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter(|s| s.host == instance::HOST)
            .map(|s| (s.id, s.time))
            .collect();
        let kept = app_repo::keep(&snapshots, config.policy, config.offset_seconds);
        let gone: std::collections::HashSet<String> = snapshots
            .into_iter()
            .map(|(id, _)| id)
            .filter(|id| !kept.contains(id))
            .collect();
        if !gone.is_empty() {
            repo.forget(&gone).map_err(|e| e.to_string())?;
            repo.prune(instance::HOST, now())
                .map_err(|e| e.to_string())?;
        }
        let count = repo.snapshots().map_err(|e| e.to_string())?.len() as u64;
        let stored = repo
            .target()
            .list("data/")
            .map_err(|e| e.to_string())?
            .iter()
            .map(|l| l.size)
            .sum();
        Ok::<_, String>((report, count, stored))
    })
    .await
    .map_err(|e| e.to_string())?;
    remove_scratch(&dir);
    result
}

/// Mails the admins about a failed backup, when mail is set up.
async fn alert(state: &AppState, why: &str) {
    if !state.mailer.enabled() {
        return;
    }
    let admins = state
        .db
        .call(|c| {
            let mut stmt = c.prepare(
                "SELECT email FROM users WHERE role = 'admin' AND email IS NOT NULL
                 AND email_verified_at IS NOT NULL",
            )?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            Ok(rows.filter_map(Result::ok).collect::<Vec<_>>())
        })
        .await
        .unwrap_or_default();
    for email in admins {
        state.mailer.send(
            &email,
            format!("{}: the backup failed", app_brand::DISPLAY_NAME),
            format!(
                "The backup of {} failed:\n\n{why}\n\nSee the admin page, Backups.",
                if state.config.server.public_url.is_empty() {
                    app_brand::DISPLAY_NAME
                } else {
                    &state.config.server.public_url
                }
            ),
        );
    }
}

/// Runs backups on their schedule: a look every five minutes, a run when a
/// period passed since the last success (a failed one tried again after
/// half an hour).
pub fn spawn_schedule(state: AppState) {
    tokio::spawn(async move {
        let mut every = tokio::time::interval(std::time::Duration::from_secs(300));
        loop {
            every.tick().await;
            let master = state.master.clone();
            let looked = state
                .db
                .call(move |c| Ok((config(c, &master)?, status(c)?)))
                .await;
            let Ok((Some(config), status)) = looked else {
                continue;
            };
            let Some(period) = config.schedule.period_ms() else {
                continue;
            };
            let now = state.clock.now();
            let since_success = status.last_success_at.map_or(i64::MAX, |t| now - t);
            let since_run = status.last_run_at.map_or(i64::MAX, |t| now - t);
            if since_success >= period
                && since_run >= period.min(30 * 60_000)
                && !running()
                && let Err(why) = run(&state, "schedule").await
            {
                tracing::warn!("backup failed: {why}");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn target(value: serde_json::Value) -> ServerTarget {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn stored_secrets_stay_only_with_the_same_place() {
        let sftp = |host: &str, user: &str, path: &str, password: &str| {
            target(
                json!({ "kind": "sftp", "host": host, "port": 22, "username": user,
                "path": path, "password": password }),
            )
        };
        let old = sftp("nas.home", "backup", "/srv/a", "secret");
        let kept = |new: ServerTarget| match new.keeping_secrets_of(&old) {
            ServerTarget::Sftp(c) => c.password,
            _ => unreachable!(),
        };
        assert_eq!(
            kept(sftp("NAS.home", "backup", "/srv/b", "")),
            "secret",
            "another folder"
        );
        assert_eq!(
            kept(sftp("evil.example", "backup", "/srv/a", "")),
            "",
            "another host"
        );
        assert_eq!(
            kept(sftp("nas.home", "root", "/srv/a", "")),
            "",
            "another user"
        );
        assert_eq!(
            kept(sftp("nas.home", "backup", "/srv/a", "new")),
            "new",
            "typed again"
        );

        let dav = |url: &str| {
            target(json!({ "kind": "webdav", "url": url, "username": "me", "password": "" }))
        };
        let old = target(
            json!({ "kind": "webdav", "url": "https://cloud.home/dav/a", "username": "me", "password": "pw" }),
        );
        let kept = |new: ServerTarget| match new.keeping_secrets_of(&old) {
            ServerTarget::Webdav(c) => c.password,
            _ => unreachable!(),
        };
        assert_eq!(kept(dav("https://cloud.home/dav/b")), "pw");
        assert_eq!(kept(dav("https://cloud.evil/dav/a")), "");
        assert_eq!(
            kept(dav("http://cloud.home/dav/a")),
            "",
            "not over plain http"
        );

        let s3 = |bucket: &str| {
            target(
                json!({ "kind": "s3", "endpoint": "https://s3.example", "bucket": bucket,
                "accessKey": "AK", "secretKey": "" }),
            )
        };
        let old = target(
            json!({ "kind": "s3", "endpoint": "https://s3.example", "bucket": "a",
            "accessKey": "AK", "secretKey": "SK" }),
        );
        let kept = |new: ServerTarget| match new.keeping_secrets_of(&old) {
            ServerTarget::S3(c) => c.secret_key,
            _ => unreachable!(),
        };
        assert_eq!(kept(s3("a")), "SK");
        assert_eq!(kept(s3("b")), "", "another bucket");
    }
}

#[cfg(test)]
mod folder_tests {
    use super::folder_allowed;

    #[test]
    fn a_folder_place_is_new_empty_or_a_backup() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            folder_allowed(&dir.path().join("new")).is_ok(),
            "not there yet"
        );
        assert!(folder_allowed(dir.path()).is_ok(), "empty");
        std::fs::write(dir.path().join("notes.txt"), b"x").unwrap();
        assert!(folder_allowed(dir.path()).is_err(), "holds other files");
        std::fs::write(dir.path().join("config"), b"{}").unwrap();
        assert!(folder_allowed(dir.path()).is_ok(), "a backup");
        assert!(folder_allowed(std::path::Path::new("relative/path")).is_err());
    }
}

#[cfg(test)]
mod sealed_tests {
    use super::*;
    use crate::db::Db;

    /// The target's secrets and the repository
    /// password are not in the database as text, and old plain settings are
    /// sealed when read.
    #[tokio::test]
    async fn backup_settings_are_sealed_in_the_database() {
        let db = Db::in_memory().unwrap();
        let master = std::sync::Arc::new(MasterKey::generate());
        let m = master.clone();
        let (raw, back, resealed) = db
            .call(move |c| {
                let settings = BackupConfig {
                    target: serde_json::from_value(
                        serde_json::json!({ "kind": "folder", "path": "/somewhere" }),
                    )
                    .unwrap(),
                    password: "the repository password".into(),
                    schedule: Schedule::Daily,
                    policy: Policy::default(),
                    offset_seconds: 0,
                };
                // as 0.2 kept them
                write_setting(c, "backups", Some(&settings))?;
                let back = config(c, &m)?;
                let raw: String = c.query_row(
                    "SELECT value FROM settings WHERE key = 'backups'",
                    [],
                    |r| r.get(0),
                )?;
                set_config(c, &m, back.as_ref())?;
                Ok((raw, back, config(c, &m)?))
            })
            .await
            .unwrap();
        assert!(!raw.contains("the repository password"), "{raw}");
        assert!(!raw.contains("somewhere"), "{raw}");
        assert_eq!(back.unwrap().password, "the repository password");
        assert_eq!(resealed.unwrap().password, "the repository password");
        let other = MasterKey::generate();
        let opened = db.call(move |c| config(c, &other)).await.unwrap();
        assert!(opened.is_none(), "another key reads nothing");
    }
}
