// SPDX-License-Identifier: AGPL-3.0-or-later

//! The data folder holds every secret the server made on its first start:
//! the master key, the databases (sign-in tokens' hashes, the OPAQUE setup,
//! the feedback key). On Unix only the server's own user may read them: the
//! folder `0700`, the files `0600`. Looser modes, left by a copy or an older
//! version, are tightened at each start and said in the log.

use std::path::Path;

/// Files in the data folder that only the server reads.
#[cfg(unix)]
const PRIVATE_FILES: &[&str] = &[
    crate::db::DB_FILE,
    "server.db-wal",
    "server.db-shm",
    crate::db::FEEDBACK_FILE,
    "feedback.db-wal",
    "feedback.db-shm",
    crate::keys::KEY_FILE,
];

/// Tightens the data folder and its secret files, returning what changed.
/// `config_file` is only checked: the server may not own it.
///
/// # Errors
///
/// A mode that cannot be read or changed.
#[cfg(unix)]
pub fn tighten(data_dir: &Path, config_file: Option<&Path>) -> std::io::Result<Vec<String>> {
    use std::os::unix::fs::PermissionsExt;
    let mut changed = Vec::new();
    let mut set = |path: &Path, mode: u32| -> std::io::Result<()> {
        let Ok(meta) = std::fs::metadata(path) else {
            return Ok(());
        };
        let current = meta.permissions().mode() & 0o777;
        if current & !mode != 0 {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
            changed.push(format!("{} {current:o} to {mode:o}", path.display()));
        }
        Ok(())
    };
    set(data_dir, 0o700)?;
    for name in PRIVATE_FILES {
        set(&data_dir.join(name), 0o600)?;
    }
    if let Some(file) = config_file
        && let Ok(meta) = std::fs::metadata(file)
        && meta.permissions().mode() & 0o077 != 0
    {
        tracing::warn!(
            "{} can be read by other users of this machine, it may hold secrets: chmod 600 it",
            file.display()
        );
    }
    Ok(changed)
}

/// Makes the data folder and tightens it before anything is made in it,
/// saying which files were loose.
///
/// # Errors
///
/// The folder cannot be made or its modes changed.
pub fn prepare(data_dir: &Path, config_file: Option<&Path>) -> Result<(), String> {
    std::fs::create_dir_all(data_dir).map_err(|e| format!("data folder: {e}"))?;
    let changed =
        tighten(data_dir, config_file).map_err(|e| format!("data folder permissions: {e}"))?;
    if !changed.is_empty() {
        tracing::warn!(
            "only the server's user may read its secrets, tightened: {}",
            changed.join(", ")
        );
    }
    Ok(())
}

/// When `folder` belongs to another user than the one the server runs as,
/// what to do about it: data copied from elsewhere (an older install, a
/// backup) keeps its old owner, and the server's user cannot open it.
#[cfg(unix)]
pub fn owner_hint(folder: &Path) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    let owner = std::fs::metadata(folder).ok()?.uid();
    // /proc/self belongs to the user the process runs as (Linux)
    let me = std::fs::metadata("/proc/self").ok()?.uid();
    (owner != me).then(|| {
        format!(
            "{} belongs to user {owner}, this server runs as user {me}: give it the folder, \
             for example chown -R {me}:{me} {}",
            folder.display(),
            folder.display()
        )
    })
}

/// Other systems have no owner to fix.
#[cfg(not(unix))]
pub fn owner_hint(_folder: &Path) -> Option<String> {
    None
}

/// `message`, followed by [`owner_hint`] when there is one.
pub fn with_owner_hint(message: &str, folder: &Path) -> String {
    match owner_hint(folder) {
        Some(hint) => format!("{message}. {hint}"),
        None => message.to_owned(),
    }
}

/// Windows folders inherit their owner's access, nothing to tighten.
#[cfg(not(unix))]
#[expect(clippy::unnecessary_wraps, reason = "the same signature as on Unix")]
pub fn tighten(_data_dir: &Path, _config_file: Option<&Path>) -> std::io::Result<Vec<String>> {
    Ok(Vec::new())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn loose_modes_are_tightened_once() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        std::fs::create_dir(&data).unwrap();
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o755)).unwrap();
        let db = data.join(crate::db::DB_FILE);
        std::fs::write(&db, "x").unwrap();
        std::fs::set_permissions(&db, std::fs::Permissions::from_mode(0o644)).unwrap();
        let changed = tighten(&data, None).unwrap();
        assert_eq!(changed.len(), 2, "{changed:?}");
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&data), 0o700);
        assert_eq!(mode(&db), 0o600);
        assert!(tighten(&data, None).unwrap().is_empty());
    }
}
