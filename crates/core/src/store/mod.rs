// SPDX-License-Identifier: AGPL-3.0-or-later

//! The local note store. One [`Store`] wraps one open database.
//!
//! Every write runs in a transaction that also updates the search index, so
//! the index can never disagree with the notes. The methods are spread over
//! the files of this module by topic.

mod attachments;
mod backup;
mod backups;
mod conflicts;
mod folders;
mod history;
mod imports;
mod links;
mod notes;
mod search;
mod spaces;
mod sync;
mod tags;
mod trash;

use std::sync::Arc;

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::clock::{Clock, SystemClock, Timestamp};
use crate::error::{Error, Result};
use crate::id::NoteId;
use crate::model::{Note, NoteSummary};

pub use attachments::{FileKeying, FileUse, Released};
pub(crate) use attachments::{FileRow, file_rows, free_name, replace_file_rows};
pub use backup::{
    BackupAttachment, BackupData, BackupFolder, BackupNote, BackupVersion, RestoreReport,
};
pub use conflicts::{NoteConflict, Resolution};
pub(crate) use history::save_text as history_save;
pub(crate) use history::snapshot_if_new_session as history_snapshot;
pub use history::{EDIT_SESSION, MAX_VERSIONS_PER_NOTE};
pub use imports::{ImportBatch, ImportFile, ImportFolder, ImportNote, ImportPlace};
pub use links::LinkSuggestion;
pub(crate) use links::rebuild_link_index;
pub use notes::{NewNote, NoteChange};
pub(crate) use reindex as reindex_note;
pub use spaces::{Item, LocalSpace, RootNote, SpaceRoots, WaitingMember};
pub use sync::{NewLink, SyncLink, Taken, Tokens};
pub(crate) use tags::replace_note_tags as replace_tags;
pub use trash::TRASH_RETENTION;

/// Local note storage.
pub struct Store {
    conn: Connection,
    clock: Arc<dyn Clock>,
    /// The sync documents, see [`crate::sync`].
    docs: crate::sync::Docs,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store").finish_non_exhaustive()
    }
}

impl Store {
    /// The connection, for the vault's own work on the file (encrypting).
    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Wraps an open database (see [`crate::db::open`]) with the real clock.
    pub fn new(conn: Connection) -> Self {
        Self::with_clock(conn, Arc::new(SystemClock))
    }

    /// Wraps an open database with a custom clock, for tests and seed data.
    pub fn with_clock(conn: Connection, clock: Arc<dyn Clock>) -> Self {
        Self {
            conn,
            clock,
            docs: crate::sync::Docs::default(),
        }
    }

    /// Swaps the clock of an already open store, for seed data spread over time.
    pub fn set_clock(&mut self, clock: Arc<dyn Clock>) {
        self.clock = clock;
    }

    /// Counts live notes, folders and tags, for status lines and dev tools.
    ///
    /// # Errors
    ///
    /// Fails only on database errors.
    pub fn counts(&self) -> Result<(i64, i64, i64)> {
        Ok(self.conn.query_row(
            "SELECT (SELECT count(*) FROM notes WHERE trashed_at IS NULL),
                    (SELECT count(*) FROM folders WHERE trashed_at IS NULL),
                    (SELECT count(*) FROM tags)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?)
    }

    fn now(&self) -> Timestamp {
        self.clock.now()
    }
}

/// Sync, see [`crate::sync`].
impl Store {
    /// Local changes not yet taken by the server, oldest first.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn sync_outbox(&mut self, limit: usize) -> Result<Vec<crate::sync::Outgoing>> {
        crate::sync::outbox(&self.conn, &mut self.docs, None, limit)
    }

    /// Documents and changes waiting for the server.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn sync_counts(&self) -> Result<crate::sync::SyncCounts> {
        crate::sync::counts(&self.conn)
    }

    /// Forgets outbox entries up to `seq`, the server has them.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn sync_ack(&mut self, up_to: i64) -> Result<()> {
        crate::sync::ack(&self.conn, None, up_to)
    }

    /// Forgets one space's outbox entries up to `seq`, the server has them.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn sync_ack_space(&mut self, space: &str, up_to: i64) -> Result<()> {
        crate::sync::ack(&self.conn, Some(space), up_to)
    }

    /// Takes a change made on another device and updates the notes, folders
    /// and tags it affects.
    ///
    /// # Errors
    ///
    /// Bytes that are not a change of this document, database errors.
    pub fn apply_remote(&mut self, doc_id: &str, bytes: &[u8]) -> Result<()> {
        let now = self.now();
        let tx = self.conn.transaction()?;
        let done = crate::sync::apply_remote(&tx, &mut self.docs, doc_id, bytes, now)
            .and_then(|()| Ok(tx.commit()?));
        if done.is_err() {
            self.docs.forget();
        }
        done
    }

    /// The whole state of a document, for a device starting from nothing.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn sync_snapshot(&mut self, doc_id: &str) -> Result<Vec<u8>> {
        crate::sync::snapshot(&self.conn, &mut self.docs, doc_id)
    }
}

/// Removes control characters other than tab and line breaks. They have no
/// meaning in notes, and keeping them out lets search use them as markers.
pub(crate) fn clean_text(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t' | '\r'))
        .collect()
}

/// Short single line preview of a body for lists.
fn preview(title: &str, body: &str) -> String {
    crate::preview::preview(title, body, 140)
}

const NOTE_COLUMNS: &str =
    "id, folder_id, title, body, pinned, created_at, updated_at, trashed_at, space_id";

fn note_from_row(row: &Row<'_>) -> rusqlite::Result<Note> {
    Ok(Note {
        id: row.get(0)?,
        folder_id: row.get(1)?,
        title: row.get(2)?,
        body: row.get(3)?,
        pinned: row.get(4)?,
        tags: Vec::new(),
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        trashed_at: row.get(7)?,
        space_id: row.get(8)?,
    })
}

fn summary_from_row(row: &Row<'_>) -> rusqlite::Result<NoteSummary> {
    let title: String = row.get(2)?;
    let body: String = row.get(3)?;
    Ok(NoteSummary {
        id: row.get(0)?,
        folder_id: row.get(1)?,
        snippet: preview(&title, &body),
        title,
        pinned: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        space_id: row.get(8)?,
    })
}

pub(crate) fn load_note(conn: &Connection, id: NoteId) -> Result<Note> {
    let mut note = conn
        .query_row(
            &format!("SELECT {NOTE_COLUMNS} FROM notes WHERE id = ?1"),
            [id],
            note_from_row,
        )
        .optional()?
        .ok_or(Error::NotFound("note"))?;
    note.tags = tags::note_tag_names(conn, id)?;
    Ok(note)
}

/// Rewrites the search entry, title key and outgoing links of one note from
/// its current row. Called after every change to a title, body or tags.
pub(crate) fn reindex(conn: &Connection, id: NoteId) -> Result<()> {
    let (rowid, title, body): (i64, String, String) = conn.query_row(
        "SELECT rowid, title, body FROM notes WHERE id = ?1",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let tags = tags::note_tag_names(conn, id)?.join(" ");
    conn.execute("DELETE FROM notes_fts WHERE rowid = ?1", [rowid])?;
    conn.execute(
        "INSERT INTO notes_fts (rowid, title, body, tags) VALUES (?1, ?2, ?3, ?4)",
        params![rowid, title, body, tags],
    )?;
    links::index_note(conn, id, &title, &body)
}

#[cfg(any(test, feature = "testing"))]
impl Store {
    /// Trashed folders, which `list_folders` leaves out.
    pub(crate) fn list_trashed_folders_for_tests(&self) -> Vec<String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, parent_id, name, position, trashed_at FROM folders WHERE trashed_at IS NOT NULL")
            .unwrap();
        stmt.query_map([], |r| {
            Ok(format!(
                "trashed folder {} parent={:?} name={:?} pos={} trashed={:?}",
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?
            ))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
    }
}

#[cfg(test)]
impl Store {
    /// As if the vault predated sync.
    pub(crate) fn wipe_sync_for_tests(&mut self) {
        self.conn
            .execute_batch(
                "DELETE FROM sync_docs; DELETE FROM sync_updates; DELETE FROM sync_outbox;
                 DELETE FROM sync_touched; DELETE FROM vault_meta WHERE key = 'sync_peer';",
            )
            .unwrap();
        self.docs.forget();
    }

    /// The migration step that records an existing vault.
    pub(crate) fn bootstrap_sync_for_tests(&mut self) {
        crate::sync::bootstrap(&self.conn).unwrap();
        self.docs.forget();
    }

    /// Tags of every entry in the space, and the tags table.
    pub(crate) fn debug_space_tags_for_tests(&mut self) -> String {
        let space = self.docs.get(&self.conn, crate::sync::SPACE).unwrap();
        let value = space.get_map("notes").get_deep_value();
        let mut stmt = self.conn.prepare("SELECT name FROM tags").unwrap();
        let tags: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        format!(
            "{value:?}
tags table: {tags:?}"
        )
    }

    /// Changes set aside, with why.
    pub(crate) fn rejected_for_tests(&self) -> Vec<String> {
        let mut stmt = self
            .conn
            .prepare("SELECT seq, doc_id, reason FROM sync_rejected")
            .unwrap();
        stmt.query_map([], |r| {
            Ok(format!(
                "{} {} {}",
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?
            ))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
    }

    /// Documents holding changes not written to the database.
    pub(crate) fn unsaved_docs_for_tests(&self) -> Vec<String> {
        self.docs.unsaved()
    }

    /// A note's file entries in a space's document.
    pub(crate) fn debug_files_for_tests(&mut self, space: &str, note: &str) -> String {
        let doc = self
            .docs
            .get(&self.conn, &crate::sync::doc_key(space, crate::sync::SPACE))
            .unwrap();
        let notes = doc.get_map("notes").get_deep_value();
        let loro::LoroValue::Map(map) = notes else {
            return String::new();
        };
        format!(
            "peer {} vv {:?} {:?}",
            doc.peer_id(),
            doc.oplog_vv(),
            map.get(note)
                .map(|v| format!("{v:?}").chars().take(200).collect::<String>())
        )
    }

    /// Drops the loaded documents, they reload from the database.
    pub(crate) fn forget_docs_for_tests(&mut self) {
        self.docs.forget();
    }

    /// Whether a document is kept here.
    pub(crate) fn doc_stored_for_tests(&self, id: &str) -> bool {
        self.docs.stored(&self.conn, id).unwrap()
    }

    /// Changes waiting for one they build on.
    pub(crate) fn waiting_for_tests(&self) -> i64 {
        self.conn
            .query_row("SELECT count(*) FROM sync_waiting", [], |row| row.get(0))
            .unwrap()
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::Arc;

    use super::Store;
    use crate::clock::ManualClock;

    /// A store on a fresh in-memory database with a hand-driven clock.
    pub fn store() -> (Store, Arc<ManualClock>) {
        let clock = Arc::new(ManualClock::new(1_700_000_000_000));
        (
            Store::with_clock(crate::db::open_in_memory(), clock.clone()),
            clock,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_text_drops_control_characters_but_keeps_lines() {
        assert_eq!(clean_text("a\u{2}b\nc\td\u{7f}"), "ab\nc\td");
    }

    #[test]
    fn preview_flattens_and_shortens() {
        assert_eq!(preview("", "one\n\ntwo   three"), "one two three");
        let long = "word ".repeat(100);
        let p = preview("", &long);
        assert!(p.ends_with('…'));
        assert_eq!(p.chars().count(), 141);
    }
}
