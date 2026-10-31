// SPDX-License-Identifier: AGPL-3.0-or-later

//! Opening the SQLite database and keeping its schema current.

use std::path::Path;

use rusqlite::Connection;

use crate::crypto::DatabaseKey;
use crate::error::{Error, Result};

/// One schema step: SQL, then optional Rust code for work SQL cannot do,
/// such as indexing existing notes with a parser.
struct Migration {
    sql: &'static str,
    then: Option<fn(&Connection) -> Result<()>>,
}

/// Schema steps, applied in order. The database `user_version` records how
/// many have run. Never edit a published step, add a new one instead.
const MIGRATIONS: &[Migration] = &[
    Migration {
        sql: include_str!("db/001_init.sql"),
        then: None,
    },
    Migration {
        sql: include_str!("db/002_links_and_blobs.sql"),
        then: Some(crate::store::rebuild_link_index),
    },
    Migration {
        sql: include_str!("db/003_sync.sql"),
        then: None,
    },
    // builds the documents once every sync table exists. A vault built
    // with step 3 already has them: recording again only writes what
    // differs, which is nothing
    Migration {
        sql: include_str!("db/004_sync_link.sql"),
        then: None,
    },
    Migration {
        sql: include_str!("db/005_conflicts.sql"),
        then: None,
    },
    Migration {
        sql: include_str!("db/006_trash_arrived.sql"),
        then: None,
    },
    Migration {
        sql: include_str!("db/007_compaction.sql"),
        then: None,
    },
    // builds the documents once every sync table has its final shape. A
    // vault built earlier already has them: recording again only writes
    // what differs, which is nothing
    Migration {
        sql: include_str!("db/008_spaces.sql"),
        then: Some(crate::sync::bootstrap),
    },
    Migration {
        sql: include_str!("db/009_key_pair.sql"),
        then: None,
    },
    // records the attachments already in a vault into its documents
    Migration {
        sql: include_str!("db/010_files.sql"),
        then: Some(crate::sync::bootstrap_files),
    },
    Migration {
        sql: include_str!("db/011_file_transfers.sql"),
        then: None,
    },
    Migration {
        sql: include_str!("db/012_imports.sql"),
        then: None,
    },
    Migration {
        sql: include_str!("db/013_plugins.sql"),
        then: None,
    },
    Migration {
        sql: include_str!("db/014_backups.sql"),
        then: None,
    },
    Migration {
        sql: include_str!("db/015_no_plugins.sql"),
        then: None,
    },
    Migration {
        sql: include_str!("db/016_space_trust.sql"),
        then: None,
    },
];

/// Opens or creates the database at `path`. With a key the file is encrypted
/// by SQLCipher, without one it is a plain SQLite file.
///
/// # Errors
///
/// [`Error::WrongKey`] when the key does not match an existing file, or when
/// an encrypted file is opened without a key.
pub fn open(path: &Path, key: Option<&DatabaseKey>) -> Result<Connection> {
    let conn = Connection::open(path)?;
    // SQLCipher writes its own errors to stderr, for example on a wrong key.
    // Those cases already come back as typed errors, so the logs are only noise.
    conn.pragma_update(None, "cipher_log_level", "NONE")?;
    if let Some(key) = key {
        conn.pragma_update(None, "key", key.pragma_value().as_str())?;
    }
    // SQLCipher only notices a wrong key on the first real read
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |row| {
        row.get::<_, i64>(0)
    })
    .map_err(|err| match err {
        rusqlite::Error::SqliteFailure(code, _)
            if code.code == rusqlite::ErrorCode::NotADatabase =>
        {
            Error::WrongKey
        }
        other => Error::Db(other),
    })?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> Result<()> {
    // WAL lets the ui read while a save is in progress, NORMAL sync is safe with WAL
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}

fn migrate(conn: &Connection) -> Result<()> {
    migrate_to(conn, MIGRATIONS.len())
}

/// Brings the schema to `target` steps in one transaction: every pending
/// step's SQL first, then their Rust code in order. The code is today's
/// (it goes through the store), so it needs today's schema: run after step
/// 8 alone, it read attachment columns that step 10 adds, and every vault
/// with notes from before phase 8 failed to open. Anything failing leaves
/// the database as it was.
fn migrate_to(conn: &Connection, target: usize) -> Result<()> {
    let current: usize = conn
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .map(|v| usize::try_from(v).unwrap_or(0))?;
    if current > MIGRATIONS.len() {
        return Err(Error::Invalid(
            "this database was written by a newer version of the app, please update".into(),
        ));
    }
    if current >= target {
        return Ok(());
    }
    let steps = &MIGRATIONS[current..target];
    let tx = conn.unchecked_transaction()?;
    for step in steps {
        tx.execute_batch(step.sql)?;
    }
    // tests stop part way to build old vaults, the code then waits for the rest
    if target == MIGRATIONS.len() {
        for then in steps.iter().filter_map(|step| step.then) {
            then(&tx)?;
        }
    }
    tx.pragma_update(
        None,
        "user_version",
        i64::try_from(target).unwrap_or(i64::MAX),
    )?;
    tx.commit()?;
    Ok(())
}

/// A fully migrated in-memory database, for tests.
#[cfg(test)]
pub(crate) fn open_in_memory() -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory database");
    conn.pragma_update(None, "foreign_keys", "ON")
        .expect("pragma");
    migrate(&conn).expect("migrations");
    conn
}

/// Number of schema steps this build knows.
pub fn schema_version() -> usize {
    MIGRATIONS.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_schema_and_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.db");
        let conn = open(&path, None).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(usize::try_from(version).unwrap(), schema_version());
        drop(conn);
        open(&path, None).unwrap();
    }

    #[test]
    fn encrypted_file_has_no_plain_header_and_needs_the_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.db");
        let key = DatabaseKey::generate().unwrap();
        let conn = open(&path, Some(&key)).unwrap();
        conn.execute(
            "INSERT INTO tags (id, name, created_at) VALUES ('t', 'tomato', 0)",
            [],
        )
        .unwrap();
        drop(conn);

        let raw = std::fs::read(&path).unwrap();
        assert!(!raw.starts_with(b"SQLite format 3"));
        assert!(!raw.windows(6).any(|w| w == b"tomato"));

        assert!(matches!(open(&path, None), Err(Error::WrongKey)));
        let other = DatabaseKey::generate().unwrap();
        assert!(matches!(open(&path, Some(&other)), Err(Error::WrongKey)));
        open(&path, Some(&key)).unwrap();
    }

    /// Fills a row with what each column's type takes, so the same code
    /// writes a note or a file into the schema of any version.
    fn insert_any(conn: &Connection, table: &str, values: &[(&str, &str)]) {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap();
        let columns: Vec<(String, String, bool)> = stmt
            .query_map([], |r| Ok((r.get(1)?, r.get(2)?, r.get::<_, i64>(3)? == 1)))
            .unwrap()
            .map(|row| row.unwrap())
            .collect();
        let (names, cells): (Vec<String>, Vec<String>) = columns
            .iter()
            .filter_map(|(name, kind, not_null)| {
                let given = values
                    .iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, v)| format!("'{v}'"));
                let filler = (*not_null).then(|| {
                    if kind.contains("INT") {
                        "0".into()
                    } else {
                        "''".into()
                    }
                });
                given.or(filler).map(|v| (name.clone(), v))
            })
            .unzip();
        conn.execute(
            &format!(
                "INSERT INTO {table} ({}) VALUES ({})",
                names.join(", "),
                cells.join(", ")
            ),
            [],
        )
        .unwrap();
    }

    #[test]
    fn a_vault_with_notes_and_files_from_any_version_upgrades() {
        const NOTE: &str = "01a16bcd-5fea-73a0-9044-6dbdccb22852";
        const FILE: &str = "01a16bcd-5fea-73a0-9044-6dbdccb22853";
        for from in 1..MIGRATIONS.len() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("old.db");
            let conn = Connection::open(&path).unwrap();
            migrate_to(&conn, from).unwrap();
            insert_any(
                &conn,
                "notes",
                &[("id", NOTE), ("title", "Kept"), ("body", "text")],
            );
            let has_files = conn
                .query_row(
                    "SELECT 1 FROM sqlite_master WHERE name = 'attachments'",
                    [],
                    |_| Ok(()),
                )
                .is_ok();
            if has_files {
                insert_any(
                    &conn,
                    "attachments",
                    &[
                        ("id", FILE),
                        ("note_id", NOTE),
                        ("file_name", "a.png"),
                        ("sha256", &"0".repeat(64)),
                    ],
                );
            }
            drop(conn);
            let conn = open(&path, None).unwrap_or_else(|e| panic!("from version {from}: {e}"));
            let title: String = conn
                .query_row("SELECT title FROM notes WHERE id = ?1", [NOTE], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(title, "Kept", "from version {from}");
        }
    }

    #[test]
    fn plugins_and_their_data_go_on_upgrade() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        let conn = Connection::open(&path).unwrap();
        // the version that still had plugins
        let with_plugins = MIGRATIONS
            .iter()
            .position(|m| m.sql.contains("CREATE TABLE plugins"))
            .unwrap()
            + 1;
        migrate_to(&conn, with_plugins).unwrap();
        insert_any(&conn, "plugins", &[("id", "count")]);
        insert_any(
            &conn,
            "plugin_data",
            &[("plugin", "count"), ("key", "k"), ("value", "v")],
        );
        drop(conn);
        let conn = open(&path, None).unwrap();
        let left: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name IN ('plugins', 'plugin_data')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn refuses_a_database_from_the_future() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.db");
        let conn = open(&path, None).unwrap();
        conn.pragma_update(None, "user_version", 999).unwrap();
        drop(conn);
        assert!(matches!(open(&path, None), Err(Error::Invalid(_))));
    }
}
