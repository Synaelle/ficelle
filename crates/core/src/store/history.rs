// SPDX-License-Identifier: AGPL-3.0-or-later

//! Version history. A version is the state of a note at the start of an
//! editing session, so typing for an hour produces one version, not thousands.

use rusqlite::{Connection, OptionalExtension, Row, params};

use super::{Store, load_note, reindex};
use crate::clock::{MINUTE, Timestamp};
use crate::error::{Error, Result};
use crate::id::{NoteId, VersionId};
use crate::model::{Note, NoteVersion, VersionReason};

/// Edits closer together than this belong to the same session.
pub const EDIT_SESSION: Timestamp = 10 * MINUTE;

/// Older versions beyond this count are dropped.
pub const MAX_VERSIONS_PER_NOTE: i64 = 200;

fn version_from_row(row: &Row<'_>) -> rusqlite::Result<NoteVersion> {
    let reason: String = row.get(5)?;
    Ok(NoteVersion {
        id: row.get(0)?,
        note_id: row.get(1)?,
        title: row.get(2)?,
        body: row.get(3)?,
        created_at: row.get(4)?,
        reason: VersionReason::parse(&reason),
    })
}

fn save(conn: &Connection, note: &Note, reason: VersionReason, now: Timestamp) -> Result<()> {
    save_text(conn, note.id, &note.title, &note.body, reason, now)
}

/// Keeps a text of a note in its history.
pub(crate) fn save_text(
    conn: &Connection,
    note: NoteId,
    title: &str,
    body: &str,
    reason: VersionReason,
    now: Timestamp,
) -> Result<()> {
    conn.execute(
        "INSERT INTO note_versions (id, note_id, title, body, created_at, reason)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![VersionId::new(), note, title, body, now, reason.as_str()],
    )?;
    conn.execute(
        "DELETE FROM note_versions WHERE note_id = ?1 AND id NOT IN (
             SELECT id FROM note_versions WHERE note_id = ?1 ORDER BY created_at DESC, id DESC LIMIT ?2
         )",
        params![note, MAX_VERSIONS_PER_NOTE],
    )?;
    Ok(())
}

/// Saves `note`, the state before an edit, when this edit starts a new
/// session: the previous change is older than [`EDIT_SESSION`], or the note
/// has no version yet (so the original text of a new note is always kept).
pub(crate) fn snapshot_if_new_session(
    conn: &Connection,
    note: &Note,
    now: Timestamp,
) -> Result<()> {
    let has_versions: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM note_versions WHERE note_id = ?1)",
        [note.id],
        |row| row.get(0),
    )?;
    // a date ahead of this device's clock came from a device whose clock is
    // fast: a new session, or none would start until that date
    if now - note.updated_at >= EDIT_SESSION || note.updated_at > now || !has_versions {
        save(conn, note, VersionReason::Edit, now)?;
    }
    Ok(())
}

impl Store {
    /// Versions of a note, newest first.
    ///
    /// # Errors
    ///
    /// Fails only on database errors.
    pub fn list_versions(&self, note: NoteId) -> Result<Vec<NoteVersion>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, note_id, title, body, created_at, reason FROM note_versions
             WHERE note_id = ?1 ORDER BY created_at DESC, id DESC",
        )?;
        let versions = stmt
            .query_map([note], version_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(versions)
    }

    /// Puts a note back to an earlier version. The current state is saved
    /// first, so a restore can itself be undone.
    ///
    /// # Errors
    ///
    /// Fails if the version does not exist or its note is in the trash.
    pub fn restore_version(&mut self, version: VersionId) -> Result<Note> {
        let now = self.now();
        let tx = self.conn.transaction()?;
        let old = tx
            .query_row(
                "SELECT id, note_id, title, body, created_at, reason FROM note_versions WHERE id = ?1",
                [version],
                version_from_row,
            )
            .optional()?
            .ok_or(Error::NotFound("version"))?;
        let current = load_note(&tx, old.note_id)?;
        if current.trashed_at.is_some() {
            return Err(Error::Invalid(
                "restore the note from the trash first".into(),
            ));
        }
        save(&tx, &current, VersionReason::BeforeRestore, now)?;
        tx.execute(
            "UPDATE notes SET title = ?2, body = ?3, updated_at = ?4 WHERE id = ?1",
            params![old.note_id, old.title, old.body, now],
        )?;
        reindex(&tx, old.note_id)?;
        crate::sync::finish(tx, &mut self.docs, self.clock.now())?;
        load_note(&self.conn, old.note_id)
    }
}

#[cfg(test)]
mod tests {
    use super::EDIT_SESSION;
    use crate::clock::MINUTE;
    use crate::model::VersionReason;
    use crate::store::test_support::store;
    use crate::store::{NewNote, NoteChange};

    fn body(text: &str) -> NoteChange {
        NoteChange {
            body: Some(text.into()),
            ..NoteChange::default()
        }
    }

    #[test]
    fn one_version_per_editing_session() {
        let (mut store, clock) = store();
        let note = store
            .create_note(&NewNote {
                body: "v0".into(),
                ..NewNote::default()
            })
            .unwrap();

        clock.advance(EDIT_SESSION);
        store.update_note(note.id, body("v1")).unwrap();
        for text in ["v2", "v3", "v4"] {
            clock.advance(MINUTE);
            store.update_note(note.id, body(text)).unwrap();
        }
        let versions = store.list_versions(note.id).unwrap();
        assert_eq!(
            versions.iter().map(|v| v.body.as_str()).collect::<Vec<_>>(),
            vec!["v0"]
        );

        clock.advance(EDIT_SESSION);
        store.update_note(note.id, body("v5")).unwrap();
        let versions = store.list_versions(note.id).unwrap();
        assert_eq!(
            versions.iter().map(|v| v.body.as_str()).collect::<Vec<_>>(),
            vec!["v4", "v0"]
        );
    }

    #[test]
    fn first_edit_right_after_creation_still_keeps_the_original() {
        let (mut store, clock) = store();
        let note = store
            .create_note(&NewNote {
                body: "original".into(),
                ..NewNote::default()
            })
            .unwrap();
        clock.advance(MINUTE);
        store.update_note(note.id, body("changed")).unwrap();
        assert_eq!(store.list_versions(note.id).unwrap()[0].body, "original");
    }

    #[test]
    fn restore_brings_back_content_and_can_be_undone() {
        let (mut store, clock) = store();
        let note = store
            .create_note(&NewNote {
                body: "old".into(),
                ..NewNote::default()
            })
            .unwrap();
        clock.advance(EDIT_SESSION);
        store.update_note(note.id, body("new")).unwrap();
        let old = store.list_versions(note.id).unwrap().pop().unwrap();

        clock.advance(MINUTE);
        let restored = store.restore_version(old.id).unwrap();
        assert_eq!(restored.body, "old");
        let undo = &store.list_versions(note.id).unwrap()[0];
        assert_eq!(undo.reason, VersionReason::BeforeRestore);
        assert_eq!(undo.body, "new");
    }
}
