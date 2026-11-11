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
    Migration {
        sql: include_str!("db/017_conflict_merged.sql"),
        then: None,
    },
    Migration {
        sql: include_str!("db/018_plain_search.sql"),
        then: Some(crate::store::rebuild_search_index),
    },
    // each folder becomes a tag holding its notes, then the documents follow
    Migration {
        sql: include_str!("db/019_nested_tags.sql"),
        then: Some(crate::store::convert_folders_to_tags),
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
/// 8 alone, it read attachment columns that step 10 adds, and every older
/// vault with notes failed to open. Anything failing leaves
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

    const FOLDER_IDS: [&str; 8] = [
        "01a16bcd-5fea-73a0-9044-6dbdccb22860",
        "01a16bcd-5fea-73a0-9044-6dbdccb22861",
        "01a16bcd-5fea-73a0-9044-6dbdccb22862",
        "01a16bcd-5fea-73a0-9044-6dbdccb22863",
        "01a16bcd-5fea-73a0-9044-6dbdccb22864",
        "01a16bcd-5fea-73a0-9044-6dbdccb22865",
        "01a16bcd-5fea-73a0-9044-6dbdccb22866",
        "01a16bcd-5fea-73a0-9044-6dbdccb22867",
    ];

    /// A vault of version `from` with nested folders, a "/" in a name, an
    /// empty and a trashed folder, and a tag named like a folder.
    fn old_vault_with_folders(path: &std::path::Path, from: usize) {
        let [
            projects,
            garden,
            slash,
            empty,
            old,
            in_garden,
            in_old,
            tagged,
        ] = FOLDER_IDS;
        let conn = Connection::open(path).unwrap();
        migrate_to(&conn, from).unwrap();
        insert_any(&conn, "folders", &[("id", projects), ("name", "Projects")]);
        insert_any(
            &conn,
            "folders",
            &[("id", garden), ("parent_id", projects), ("name", "Garden")],
        );
        insert_any(&conn, "folders", &[("id", slash), ("name", "Rain/Sun")]);
        insert_any(&conn, "folders", &[("id", empty), ("name", "Empty")]);
        insert_any(
            &conn,
            "folders",
            &[("id", old), ("name", "Old"), ("trashed_at", "5")],
        );
        insert_any(
            &conn,
            "notes",
            &[("id", in_garden), ("folder_id", garden), ("title", "Beans")],
        );
        insert_any(
            &conn,
            "notes",
            &[
                ("id", in_old),
                ("folder_id", old),
                ("title", "Gone"),
                ("trashed_at", "5"),
            ],
        );
        insert_any(&conn, "notes", &[("id", tagged), ("title", "Plan")]);
        insert_any(
            &conn,
            "tags",
            &[("id", FOLDER_IDS[0]), ("name", "projects")],
        );
        insert_any(
            &conn,
            "note_tags",
            &[("note_id", tagged), ("tag_id", FOLDER_IDS[0])],
        );
    }

    #[test]
    fn folders_become_tags_holding_their_notes_from_any_version() {
        let [_, _, _, _, _, in_garden, in_old, tagged] = FOLDER_IDS;
        for from in 1..MIGRATIONS.len() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("old.db");
            old_vault_with_folders(&path, from);

            let conn = open(&path, None).unwrap_or_else(|e| panic!("from version {from}: {e}"));
            let tags_of = |note: &str| -> Vec<String> {
                let mut stmt = conn
                    .prepare(
                        "SELECT t.name FROM tags t JOIN note_tags nt ON nt.tag_id = t.id
                         WHERE nt.note_id = ?1 ORDER BY t.name",
                    )
                    .unwrap();
                stmt.query_map([note], |r| r.get(0))
                    .unwrap()
                    .map(|r| r.unwrap())
                    .collect()
            };
            let all = |conn: &Connection| -> Vec<(String, bool)> {
                let mut stmt = conn
                    .prepare("SELECT name, explicit FROM tags ORDER BY name")
                    .unwrap();
                stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                    .unwrap()
                    .map(|r| r.unwrap())
                    .collect()
            };
            assert_eq!(tags_of(in_garden), ["Projects/Garden"], "from {from}");
            assert_eq!(tags_of(in_old), ["Old"], "from {from}");
            // the tag typed before and the folder of the same name are one
            assert_eq!(tags_of(tagged), ["projects"], "from {from}");
            let tags = all(&conn);
            assert_eq!(
                tags,
                [
                    ("Empty", true),
                    ("Old", false),
                    ("projects", true),
                    ("Projects/Garden", true),
                    ("Rain\u{2215}Sun", true),
                ]
                .map(|(n, e)| (n.to_string(), e)),
                "from {from}"
            );
            // a second run changes nothing
            crate::store::convert_folders_to_tags(&conn).unwrap();
            assert_eq!(all(&conn), tags, "from {from}");
        }
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
