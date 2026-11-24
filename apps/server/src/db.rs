// SPDX-License-Identifier: AGPL-3.0-or-later

//! Server database: one SQLite file in WAL mode under the data folder.
//! Migrations are numbered SQL files, applied in order at start and tracked
//! with `PRAGMA user_version`, each in its own transaction. Calls run on
//! tokio's blocking pool so a slow query never stalls the async workers.
//! A storage trait for Postgres comes when there is a second backend.

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use rusqlite::Connection;

/// File name inside `storage.data_dir`.
pub const DB_FILE: &str = "server.db";

/// The feedback reports' own file, kept apart from accounts and notes.
pub const FEEDBACK_FILE: &str = "feedback.db";

/// Applied in order, the position is the schema version.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/001_init.sql"),
    include_str!("../migrations/002_accounts.sql"),
    include_str!("../migrations/003_groups.sql"),
    include_str!("../migrations/004_second_factor.sql"),
    include_str!("../migrations/005_passkeys.sql"),
    include_str!("../migrations/006_sso.sql"),
    include_str!("../migrations/007_e2ee.sql"),
    include_str!("../migrations/008_email_and_deletion.sql"),
    include_str!("../migrations/009_sync.sql"),
    include_str!("../migrations/010_compaction.sql"),
    include_str!("../migrations/011_sharing.sql"),
    include_str!("../migrations/012_files.sql"),
    include_str!("../migrations/013_policies.sql"),
    include_str!("../migrations/014_extensions.sql"),
    include_str!("../migrations/015_feedback.sql"),
    include_str!("../migrations/016_setup.sql"),
    include_str!("../migrations/017_factor_required.sql"),
    include_str!("../migrations/018_no_plugins.sql"),
    include_str!("../migrations/019_ksf.sql"),
    include_str!("../migrations/020_second_step.sql"),
    include_str!("../migrations/021_web_version.sql"),
    include_str!("../migrations/022_app_codes.sql"),
    include_str!("../migrations/023_app_links.sql"),
    include_str!("../migrations/024_link_offers.sql"),
    include_str!("../migrations/025_sign_in_locks.sql"),
    include_str!("../migrations/026_link_networks.sql"),
    include_str!("../migrations/027_one_person.sql"),
    include_str!("../migrations/028_leftovers.sql"),
];

/// The feedback database's migrations.
const FEEDBACK_MIGRATIONS: &[&str] = &[
    include_str!("../migrations/feedback/001_init.sql"),
    include_str!("../migrations/feedback/002_about.sql"),
    include_str!("../migrations/feedback/003_server.sql"),
    include_str!("../migrations/feedback/004_answers.sql"),
    include_str!("../migrations/feedback/005_no_keeping.sql"),
];

/// Shared handle to the database.
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

/// A failed database call.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    /// SQLite reported an error.
    #[error("database: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// The data folder cannot be created.
    #[error("cannot create {0}: {1}")]
    Folder(String, std::io::Error),
    /// The file was written by a newer server.
    #[error(
        "the database is at schema {found}, this server knows up to {known}: a newer version \
         wrote it. Run that version, or to go back to this one restore the backup made \
         before updating"
    )]
    TooNew {
        /// Version in the file.
        found: usize,
        /// Newest version this build has.
        known: usize,
    },
    /// Stored json that does not read back.
    #[error("stored data: {0}")]
    Data(String),
    /// The blocking task panicked or was cancelled.
    #[error("database task failed: {0}")]
    Task(String),
}

impl Db {
    /// Opens (or creates) the database in `dir` and brings its schema up to
    /// date.
    ///
    /// # Errors
    ///
    /// Fails when the folder or file cannot be opened, a migration fails, or
    /// the file comes from a newer server.
    pub fn open(dir: &Path) -> Result<Self, DbError> {
        Self::open_file(dir, DB_FILE, MIGRATIONS)
    }

    /// Opens (or creates) the feedback database in `dir`.
    ///
    /// # Errors
    ///
    /// As [`Db::open`].
    pub fn open_feedback(dir: &Path) -> Result<Self, DbError> {
        Self::open_file(dir, FEEDBACK_FILE, FEEDBACK_MIGRATIONS)?.zeroing_deletes()
    }

    /// Deleted rows are overwritten, not left readable in free pages: a
    /// deleted report must be gone from the file.
    fn zeroing_deletes(self) -> Result<Self, DbError> {
        self.conn
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pragma_update(None, "secure_delete", "ON")?;
        Ok(self)
    }

    fn open_file(dir: &Path, file: &str, migrations: &[&str]) -> Result<Self, DbError> {
        std::fs::create_dir_all(dir).map_err(|e| DbError::Folder(dir.display().to_string(), e))?;
        let conn = Connection::open(dir.join(file))?;
        Self::setup(conn, migrations)
    }

    /// A private database in memory, for tests.
    #[cfg(test)]
    ///
    /// # Errors
    ///
    /// Fails only if a migration is broken.
    pub fn in_memory() -> Result<Self, DbError> {
        Self::setup(Connection::open_in_memory()?, MIGRATIONS)
    }

    /// A private feedback database in memory, for tests.
    ///
    /// # Errors
    ///
    /// Fails only if a migration is broken.
    #[cfg(test)]
    pub fn feedback_in_memory() -> Result<Self, DbError> {
        Self::setup(Connection::open_in_memory()?, FEEDBACK_MIGRATIONS)?.zeroing_deletes()
    }

    fn setup(mut conn: Connection, migrations: &[&str]) -> Result<Self, DbError> {
        // WAL lets readers run while one writer commits, NORMAL sync is safe in WAL
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        migrate(&mut conn, migrations)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Runs `f` with the connection on the blocking pool.
    ///
    /// # Errors
    ///
    /// Whatever `f` returns, or a task failure.
    pub async fn call<T, F>(&self, f: F) -> Result<T, DbError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, DbError> + Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            // a panic in an earlier call cannot leave sqlite half written,
            // every write runs in a transaction, so the connection is fine
            let mut guard = conn.lock().unwrap_or_else(PoisonError::into_inner);
            f(&mut guard)
        })
        .await
        .map_err(|e| DbError::Task(e.to_string()))?
    }
}

/// A migrated feedback connection in memory, for tests of the store.
#[cfg(test)]
pub fn feedback_connection() -> Connection {
    let mut conn = Connection::open_in_memory().expect("sqlite opens in memory");
    conn.pragma_update(None, "foreign_keys", "ON")
        .expect("pragmas apply");
    migrate(&mut conn, FEEDBACK_MIGRATIONS).expect("the migrations apply");
    conn
}

fn migrate(conn: &mut Connection, migrations: &[&str]) -> Result<(), DbError> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    // a negative version is not ours either, treat it as unknown and newer
    let current = usize::try_from(version).unwrap_or(usize::MAX);
    if current > migrations.len() {
        return Err(DbError::TooNew {
            found: current,
            known: migrations.len(),
        });
    }
    for (index, sql) in migrations.iter().enumerate().skip(current) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(
            None,
            "user_version",
            i64::try_from(index + 1).unwrap_or(i64::MAX),
        )?;
        tx.commit()?;
        tracing::info!("database schema {}", index + 1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_new_database_gets_every_migration_and_an_instance_id() {
        let db = Db::in_memory().unwrap();
        let (version, id) = db
            .call(|c| {
                let version: i64 = c.pragma_query_value(None, "user_version", |r| r.get(0))?;
                let id: String =
                    c.query_row("SELECT value FROM instance WHERE key = 'id'", [], |r| {
                        r.get(0)
                    })?;
                Ok((version, id))
            })
            .await
            .unwrap();
        assert_eq!(usize::try_from(version).unwrap(), MIGRATIONS.len());
        assert_eq!(id.len(), 32);
    }

    #[test]
    fn reopening_keeps_data_and_a_newer_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let first = Db::open(dir.path()).unwrap();
        let id = |db: &Db| -> String {
            let conn = db.conn.lock().unwrap();
            conn.query_row("SELECT value FROM instance WHERE key = 'id'", [], |r| {
                r.get(0)
            })
            .unwrap()
        };
        let before = id(&first);
        drop(first);
        let again = Db::open(dir.path()).unwrap();
        assert_eq!(id(&again), before, "migrations ran only once");
        again
            .conn
            .lock()
            .unwrap()
            .pragma_update(None, "user_version", 99)
            .unwrap();
        drop(again);
        let err = Db::open(dir.path()).err().unwrap();
        assert!(matches!(err, DbError::TooNew { found: 99, .. }), "{err}");
    }

    #[test]
    fn uploaded_plugins_go_on_upgrade_and_themes_stay() {
        let dir = tempfile::tempdir().unwrap();
        // the version that still took plugins
        let before = MIGRATIONS
            .iter()
            .position(|m| m.contains("DELETE FROM extensions WHERE kind = 'plugin'"))
            .unwrap();
        let old = Db::open_file(dir.path(), DB_FILE, &MIGRATIONS[..before]).unwrap();
        {
            let conn = old.conn.lock().unwrap();
            for kind in ["plugin", "theme"] {
                conn.execute(
                    "INSERT INTO extensions (kind, id, name, meta, content, sha256, preinstall, uploaded_at)
                     VALUES (?1, 'x', 'X', '{}', x'00', '00', 0, 0)",
                    [kind],
                )
                .unwrap();
            }
        }
        drop(old);
        let db = Db::open_file(dir.path(), DB_FILE, &MIGRATIONS[..=before]).unwrap();
        let kinds: Vec<String> = db
            .conn
            .lock()
            .unwrap()
            .prepare("SELECT kind FROM extensions")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(kinds, ["theme"]);
    }

    #[test]
    fn what_only_several_people_used_goes_and_the_owner_s_notes_stay() {
        let dir = tempfile::tempdir().unwrap();
        // main before servers were for one person in code too
        let before = MIGRATIONS
            .iter()
            .position(|m| m.contains("DROP TABLE invites;"))
            .unwrap();
        let old = Db::open_file(dir.path(), DB_FILE, &MIGRATIONS[..before]).unwrap();
        old.conn
            .lock()
            .unwrap()
            .execute_batch(
                "INSERT INTO users (id, username, display_name, password_hash, role, created_at)
                     VALUES ('o', 'owner', 'Owner', 'h', 'admin', 1), ('f', 'friend', 'Friend', 'h', 'user', 1);
                 INSERT INTO sync_spaces (id, owner_id, created_at) VALUES ('o', 'o', 1), ('f', 'f', 1);
                 INSERT INTO sync_spaces (id, owner_id, created_at, shared) VALUES ('s', 'o', 2, 1);
                 INSERT INTO sync_changes (space_id, doc_id, change_id, bytes, size, created_at)
                     VALUES ('o', 'space', 'c1', x'01', 1, 1), ('s', 'space', 'c2', x'02', 1, 2),
                            ('f', 'space', 'c3', x'03', 1, 3);
                 INSERT INTO files (object, size, stored, created_at) VALUES ('obj', 1, 1, 1);
                 INSERT INTO file_refs (space_id, address, object, size, added_at) VALUES ('o', 'a', 'obj', 1, 1);
                 INSERT INTO groups (id, name, created_at) VALUES ('g', 'Family', 1);
                 INSERT INTO group_members (group_id, user_id, added_at) VALUES ('g', 'f', 1);
                 INSERT INTO sync_members (space_id, user_id, role, added_at) VALUES ('s', 'f', 'edit', 1);
                 INSERT INTO sync_members (space_id, group_id, role, added_at) VALUES ('s', 'g', 'read', 1);
                 INSERT INTO sync_left (space_id, user_id) VALUES ('s', 'f');
                 INSERT INTO sync_grants (space_id, user_id, epoch, grant_data) VALUES ('s', 'f', 0, 'k');
                 INSERT INTO invites (id, hash, created_by, created_at, expires_at) VALUES ('i', x'00', 'o', 1, 2);
                 INSERT INTO sso_identities (id, user_id, issuer, subject, name, created_at)
                     VALUES ('x', 'f', 'https://id', 'sub', 'friend', 1);
                 INSERT INTO feedback_readers (user_id, added_at) VALUES ('f', 1);
                 INSERT INTO policies (scope, target, settings, updated_at)
                     VALUES ('instance', '', '{}', 1), ('group', 'g', '{}', 1), ('user', 'f', '{}', 1);
                 UPDATE settings SET value = 'open' WHERE key = 'registration';
                 INSERT OR REPLACE INTO settings (key, value) VALUES ('server_use', 'personal'), ('account_kinds', 'e2ee');",
            )
            .unwrap();
        drop(old);
        let db = Db::open(dir.path()).unwrap();
        let conn = db.conn.lock().unwrap();
        let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(count("SELECT count(*) FROM users"), 2, "no account goes");
        assert_eq!(count("SELECT count(*) FROM sync_spaces"), 3);
        assert_eq!(
            count("SELECT count(*) FROM sync_changes"),
            3,
            "every note change stays"
        );
        assert_eq!(count("SELECT count(*) FROM file_refs"), 1);
        assert_eq!(count("SELECT count(*) FROM files"), 1);
        assert_eq!(
            count("SELECT count(*) FROM policies"),
            1,
            "the instance's stays"
        );
        assert_eq!(
            count(
                "SELECT count(*) FROM settings WHERE key IN ('registration', 'server_use', 'account_kinds')"
            ),
            0
        );
        for table in [
            "groups",
            "group_members",
            "invites",
            "sso_identities",
            "sync_members",
            "sync_left",
            "sync_grants",
            "feedback_readers",
        ] {
            let found = count(&format!(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = '{table}'"
            ));
            assert_eq!(found, 0, "{table}");
        }
        let problems: Vec<String> = conn
            .prepare("PRAGMA foreign_key_check")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn leftovers_of_sharing_go_and_the_instance_policy_stays() {
        let dir = tempfile::tempdir().unwrap();
        let old = Db::open_file(dir.path(), DB_FILE, &MIGRATIONS[..MIGRATIONS.len() - 1]).unwrap();
        old.conn
            .lock()
            .unwrap()
            .execute_batch(
                "INSERT INTO users (id, username, display_name, password_hash, role, created_at, share_public)
                     VALUES ('o', 'owner', 'Owner', 'h', 'admin', 1, 'pk');
                 INSERT INTO sync_spaces (id, owner_id, created_at, shared, epoch) VALUES ('o', 'o', 1, 1, 3);
                 INSERT INTO sync_changes (space_id, doc_id, change_id, bytes, size, created_at)
                     VALUES ('o', 'space', 'c1', x'01', 1, 1);
                 INSERT INTO extensions (kind, id, name, meta, content, sha256, preinstall, uploaded_at)
                     VALUES ('theme', 'x', 'X', '{}', x'00', '00', 0, 0);
                 INSERT INTO policies (scope, target, settings, updated_at)
                     VALUES ('instance', '', '{\"max_file_mb\":5}', 1);",
            )
            .unwrap();
        drop(old);
        let db = Db::open(dir.path()).unwrap();
        let conn = db.conn.lock().unwrap();
        let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(
            count("SELECT count(*) FROM sqlite_master WHERE name = 'extensions'"),
            0
        );
        assert_eq!(
            count(
                "SELECT count(*) FROM pragma_table_info('sync_spaces') WHERE name IN ('shared', 'epoch')"
            ),
            0
        );
        assert_eq!(
            count("SELECT count(*) FROM pragma_table_info('users') WHERE name LIKE 'share_%'"),
            0
        );
        assert_eq!(
            count("SELECT count(*) FROM sync_changes"),
            1,
            "the notes stay"
        );
        let settings: String = conn
            .query_row(
                "SELECT settings FROM policies WHERE scope = 'instance'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(settings, r#"{"max_file_mb":5}"#);
        assert!(
            conn.execute(
                "INSERT INTO policies (scope, target, settings, updated_at) VALUES ('user', 'o', '{}', 1)",
                [],
            )
            .is_err(),
            "only the instance has a policy"
        );
    }
}
