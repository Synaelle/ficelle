// SPDX-License-Identifier: AGPL-3.0-or-later

//! Trash. Deleting always goes through the trash first, items only disappear
//! when the trash is emptied or they are older than [`TRASH_RETENTION`].
//!
//! Rules:
//! - Trashing a folder trashes everything below it with the same timestamp.
//! - Restoring a folder brings back what was trashed with it, not items that
//!   were trashed on their own before.
//! - Restoring an item whose parent folder is still in the trash puts it at
//!   the top level, so it is visible again.
//!
//! The one exception: a note left blank (no title, no text, not pinned) holds
//! nothing worth a trip through the trash and is deleted straight away.

use rusqlite::{Connection, params};

use super::{Store, folders};
use crate::clock::{DAY, Timestamp};
use crate::error::{Error, Result};
use crate::id::{FolderId, NoteId};

/// How long items stay in the trash before automatic removal.
pub const TRASH_RETENTION: Timestamp = 30 * DAY;

fn parent_is_trashed(conn: &Connection, parent: Option<FolderId>) -> Result<bool> {
    match parent {
        Some(parent) => Ok(folders::load(conn, parent)?.trashed_at.is_some()),
        None => Ok(false),
    }
}

/// Live notes with nothing in them. Whitespace alone counts as nothing, a
/// pinned note was kept on purpose.
const BLANK_NOTES: &str = "SELECT id FROM notes WHERE trashed_at IS NULL AND pinned = 0
     AND trim(title, char(32, 9, 10, 13)) = '' AND trim(body, char(32, 9, 10, 13)) = ''";

/// Deletes notes and folders whose ids are listed. Notes go first because a
/// folder delete would only detach them.
fn delete_items(conn: &Connection, notes: &[NoteId], folders: &[FolderId]) -> Result<()> {
    for note in notes {
        conn.execute(
            "DELETE FROM notes_fts WHERE rowid = (SELECT rowid FROM notes WHERE id = ?1)",
            [note],
        )?;
        conn.execute("DELETE FROM notes WHERE id = ?1", [note])?;
    }
    for folder in folders {
        conn.execute("DELETE FROM folders WHERE id = ?1", [folder])?;
    }
    conn.execute(
        "DELETE FROM tags WHERE id NOT IN (SELECT tag_id FROM note_tags)",
        [],
    )?;
    Ok(())
}

impl Store {
    /// Moves a note to the trash.
    ///
    /// # Errors
    ///
    /// Fails if the note does not exist.
    pub fn trash_note(&mut self, id: NoteId) -> Result<()> {
        let now = self.now();
        let tx = self.conn.transaction()?;
        let changed = tx.execute(
            "UPDATE notes SET trashed_at = ?2 WHERE id = ?1 AND trashed_at IS NULL",
            params![id, now],
        )?;
        if changed == 0 {
            super::load_note(&tx, id)?;
        }
        crate::sync::finish(tx, &mut self.docs, now)
    }

    /// Takes a note out of the trash.
    ///
    /// # Errors
    ///
    /// Fails if the note does not exist.
    pub fn restore_note(&mut self, id: NoteId) -> Result<()> {
        let tx = self.conn.transaction()?;
        let note = super::load_note(&tx, id)?;
        let folder = if parent_is_trashed(&tx, note.folder_id)? {
            None
        } else {
            note.folder_id
        };
        tx.execute(
            "UPDATE notes SET trashed_at = NULL, folder_id = ?2 WHERE id = ?1",
            params![id, folder],
        )?;
        // brought back: the question of a note edited after its deletion
        // is answered
        tx.execute(
            "DELETE FROM note_conflicts WHERE note_id = ?1 AND kind = 'editedAfterDelete'",
            [id],
        )?;
        crate::sync::finish(tx, &mut self.docs, self.clock.now())?;
        Ok(())
    }

    /// Moves a folder and everything below it to the trash.
    ///
    /// # Errors
    ///
    /// Fails if the folder does not exist.
    pub fn trash_folder(&mut self, id: FolderId) -> Result<()> {
        let now = self.now();
        let tx = self.conn.transaction()?;
        for folder in folders::subtree(&tx, id)? {
            tx.execute(
                "UPDATE folders SET trashed_at = ?2 WHERE id = ?1 AND trashed_at IS NULL",
                params![folder, now],
            )?;
            tx.execute(
                "UPDATE notes SET trashed_at = ?2 WHERE folder_id = ?1 AND trashed_at IS NULL",
                params![folder, now],
            )?;
        }
        folders::load(&tx, id)?;
        crate::sync::finish(tx, &mut self.docs, self.clock.now())?;
        Ok(())
    }

    /// Takes a folder out of the trash with everything trashed together with it.
    ///
    /// # Errors
    ///
    /// Fails if the folder does not exist or is not in the trash.
    pub fn restore_folder(&mut self, id: FolderId) -> Result<()> {
        let tx = self.conn.transaction()?;
        let folder = folders::load(&tx, id)?;
        let Some(when) = folder.trashed_at else {
            return Err(Error::Invalid("this folder is not in the trash".into()));
        };
        for item in folders::subtree(&tx, id)? {
            tx.execute(
                "UPDATE folders SET trashed_at = NULL WHERE id = ?1 AND trashed_at = ?2",
                params![item, when],
            )?;
            tx.execute(
                "UPDATE notes SET trashed_at = NULL WHERE folder_id = ?1 AND trashed_at = ?2",
                params![item, when],
            )?;
        }
        if parent_is_trashed(&tx, folder.parent_id)? {
            tx.execute("UPDATE folders SET parent_id = NULL WHERE id = ?1", [id])?;
        }
        crate::sync::finish(tx, &mut self.docs, self.clock.now())?;
        Ok(())
    }

    /// Deletes a note that is in the trash, with its versions and attachments.
    ///
    /// # Errors
    ///
    /// Fails if the note is not in the trash.
    pub fn purge_note(&mut self, id: NoteId) -> Result<()> {
        let tx = self.conn.transaction()?;
        if super::load_note(&tx, id)?.trashed_at.is_none() {
            return Err(Error::Invalid(
                "only notes in the trash can be deleted for good".into(),
            ));
        }
        delete_items(&tx, &[id], &[])?;
        crate::sync::finish(tx, &mut self.docs, self.clock.now())?;
        Ok(())
    }

    /// Deletes a trashed folder and everything below it.
    ///
    /// # Errors
    ///
    /// Fails if the folder is not in the trash.
    pub fn purge_folder(&mut self, id: FolderId) -> Result<()> {
        let tx = self.conn.transaction()?;
        if folders::load(&tx, id)?.trashed_at.is_none() {
            return Err(Error::Invalid(
                "only folders in the trash can be deleted for good".into(),
            ));
        }
        let subtree = folders::subtree(&tx, id)?;
        let mut notes = Vec::new();
        for folder in &subtree {
            let mut stmt = tx.prepare("SELECT id FROM notes WHERE folder_id = ?1")?;
            notes.extend(
                stmt.query_map([folder], |row| row.get(0))?
                    .collect::<rusqlite::Result<Vec<NoteId>>>()?,
            );
        }
        delete_items(&tx, &notes, &subtree)?;
        crate::sync::finish(tx, &mut self.docs, self.clock.now())?;
        Ok(())
    }

    /// Deletes everything that went into the trash before `cutoff`, and
    /// returns how many notes and folders were removed. Pass the current time
    /// to empty the whole trash.
    ///
    /// # Errors
    ///
    /// Fails only on database errors.
    pub fn purge_trashed_before(&mut self, cutoff: Timestamp) -> Result<usize> {
        self.purge_trash(cutoff, false)
    }

    /// Deletes items in the trash since `cutoff`. By arrival: an item
    /// trashed on another device counts from when it reached this one.
    fn purge_trash(&mut self, cutoff: Timestamp, by_arrival: bool) -> Result<usize> {
        let since = |kind: &str| {
            if by_arrival {
                format!(
                    "coalesce((SELECT at FROM trash_arrived a WHERE a.kind = '{kind}' AND a.id = t.id), t.trashed_at)"
                )
            } else {
                "t.trashed_at".to_owned()
            }
        };
        let tx = self.conn.transaction()?;
        let notes = {
            let mut stmt = tx.prepare(&format!(
                "SELECT id FROM notes t WHERE t.trashed_at IS NOT NULL AND {} <= ?1",
                since("note")
            ))?;
            stmt.query_map([cutoff], |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<NoteId>>>()?
        };
        let folders = {
            let mut stmt = tx.prepare(&format!(
                "SELECT id FROM folders t WHERE t.trashed_at IS NOT NULL AND {} <= ?1",
                since("folder")
            ))?;
            stmt.query_map([cutoff], |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<FolderId>>>()?
        };
        // a note restored out of a trashed folder keeps living at the top level
        for folder in &folders {
            tx.execute(
                "UPDATE notes SET folder_id = NULL WHERE trashed_at IS NULL AND folder_id = ?1",
                [folder],
            )?;
        }
        delete_items(&tx, &notes, &folders)?;
        crate::sync::finish(tx, &mut self.docs, self.clock.now())?;
        Ok(notes.len() + folders.len())
    }

    /// Deletes the note if it was left blank, see [`BLANK_NOTES`]. Returns
    /// false, keeping the note, when it holds anything.
    ///
    /// # Errors
    ///
    /// Fails only on database errors.
    pub fn discard_if_blank(&mut self, id: NoteId) -> Result<bool> {
        let tx = self.conn.transaction()?;
        let blank: bool = tx.query_row(
            &format!("SELECT EXISTS ({BLANK_NOTES} AND id = ?1)"),
            [id],
            |row| row.get(0),
        )?;
        if blank {
            delete_items(&tx, &[id], &[])?;
        }
        crate::sync::finish(tx, &mut self.docs, self.clock.now())?;
        Ok(blank)
    }

    /// Deletes every note left blank, the ones abandoned when the app closed
    /// or locked with an empty note open. Call it at startup.
    ///
    /// # Errors
    ///
    /// Fails only on database errors.
    pub fn discard_blank_notes(&mut self) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let notes = {
            let mut stmt = tx.prepare(BLANK_NOTES)?;
            stmt.query_map([], |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<NoteId>>>()?
        };
        delete_items(&tx, &notes, &[])?;
        crate::sync::finish(tx, &mut self.docs, self.clock.now())?;
        Ok(notes.len())
    }

    /// Applies [`TRASH_RETENTION`]. Call it at startup.
    ///
    /// # Errors
    ///
    /// Fails only on database errors.
    pub fn purge_expired_trash(&mut self) -> Result<usize> {
        let cutoff = self.now() - TRASH_RETENTION;
        self.purge_trash(cutoff, true)
    }
}

#[cfg(test)]
mod tests {
    use crate::clock::DAY;
    use crate::model::{NoteScope, NoteSort};
    use crate::store::NewNote;
    use crate::store::test_support::store;

    #[test]
    fn trashed_notes_leave_lists_and_come_back() {
        let (mut store, _) = store();
        let note = store.create_note(&NewNote::default()).unwrap();
        store.trash_note(note.id).unwrap();
        assert!(
            store
                .list_notes(NoteScope::All, NoteSort::default())
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            store
                .list_notes(NoteScope::Trash, NoteSort::default())
                .unwrap()
                .len(),
            1
        );
        store.restore_note(note.id).unwrap();
        assert_eq!(
            store
                .list_notes(NoteScope::All, NoteSort::default())
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn folder_trash_takes_the_subtree_and_restores_only_what_went_with_it() {
        let (mut store, clock) = store();
        let a = store.create_folder(None, "a").unwrap();
        let b = store.create_folder(Some(a.id), "b").unwrap();
        let early = store
            .create_note(&NewNote {
                folder_id: Some(b.id),
                ..NewNote::default()
            })
            .unwrap();
        let later = store
            .create_note(&NewNote {
                folder_id: Some(b.id),
                ..NewNote::default()
            })
            .unwrap();

        store.trash_note(early.id).unwrap();
        clock.advance(1000);
        store.trash_folder(a.id).unwrap();
        assert!(store.note(later.id).unwrap().trashed_at.is_some());
        assert!(store.folder(b.id).unwrap().trashed_at.is_some());

        store.restore_folder(a.id).unwrap();
        assert!(store.note(later.id).unwrap().trashed_at.is_none());
        assert!(store.folder(b.id).unwrap().trashed_at.is_none());
        assert!(
            store.note(early.id).unwrap().trashed_at.is_some(),
            "trashed on its own before"
        );
    }

    #[test]
    fn restoring_a_note_from_a_trashed_folder_puts_it_at_the_top() {
        let (mut store, _) = store();
        let folder = store.create_folder(None, "f").unwrap();
        let note = store
            .create_note(&NewNote {
                folder_id: Some(folder.id),
                ..NewNote::default()
            })
            .unwrap();
        store.trash_folder(folder.id).unwrap();
        store.restore_note(note.id).unwrap();
        assert_eq!(store.note(note.id).unwrap().folder_id, None);
    }

    #[test]
    fn purge_needs_the_trash_and_removes_everything() {
        let (mut store, _) = store();
        let note = store
            .create_note(&NewNote {
                tags: vec!["solo".into()],
                ..NewNote::default()
            })
            .unwrap();
        assert!(store.purge_note(note.id).is_err());
        store.trash_note(note.id).unwrap();
        store.purge_note(note.id).unwrap();
        assert!(store.note(note.id).is_err());
        assert!(store.list_tags().unwrap().is_empty());
    }

    #[test]
    fn blank_notes_are_discarded_and_anything_written_is_kept() {
        let (mut store, _) = store();
        let blank = store
            .create_note(&NewNote {
                body: " \n\n".into(),
                tags: vec!["scope".into()],
                ..NewNote::default()
            })
            .unwrap();
        let titled = store
            .create_note(&NewNote {
                title: "Shopping".into(),
                ..NewNote::default()
            })
            .unwrap();
        let written = store
            .create_note(&NewNote {
                body: "milk".into(),
                ..NewNote::default()
            })
            .unwrap();
        let pinned = store
            .create_note(&NewNote {
                pinned: true,
                ..NewNote::default()
            })
            .unwrap();

        for kept in [titled.id, written.id, pinned.id] {
            assert!(!store.discard_if_blank(kept).unwrap());
            assert!(store.note(kept).is_ok());
        }
        assert!(store.discard_if_blank(blank.id).unwrap());
        assert!(store.note(blank.id).is_err());
        assert!(
            store.list_tags().unwrap().is_empty(),
            "its tag went with it"
        );
    }

    #[test]
    fn startup_discards_every_blank_note_but_not_the_trash() {
        let (mut store, _) = store();
        store.create_note(&NewNote::default()).unwrap();
        store.create_note(&NewNote::default()).unwrap();
        let trashed = store.create_note(&NewNote::default()).unwrap();
        store.trash_note(trashed.id).unwrap();
        store
            .create_note(&NewNote {
                title: "kept".into(),
                ..NewNote::default()
            })
            .unwrap();
        assert_eq!(store.discard_blank_notes().unwrap(), 2);
        assert!(
            store.note(trashed.id).is_ok(),
            "the trash keeps its own rules"
        );
    }

    #[test]
    fn expired_trash_is_purged_and_recent_trash_is_kept() {
        let (mut store, clock) = store();
        let old = store.create_note(&NewNote::default()).unwrap();
        store.trash_note(old.id).unwrap();
        clock.advance(20 * DAY);
        let recent = store.create_note(&NewNote::default()).unwrap();
        store.trash_note(recent.id).unwrap();
        clock.advance(11 * DAY);
        assert_eq!(store.purge_expired_trash().unwrap(), 1);
        assert!(store.note(old.id).is_err());
        assert!(store.note(recent.id).is_ok());
    }

    #[test]
    fn purging_a_folder_removes_its_notes() {
        let (mut store, _) = store();
        let folder = store.create_folder(None, "f").unwrap();
        let note = store
            .create_note(&NewNote {
                folder_id: Some(folder.id),
                ..NewNote::default()
            })
            .unwrap();
        store.trash_folder(folder.id).unwrap();
        store.purge_folder(folder.id).unwrap();
        assert!(store.note(note.id).is_err());
        assert!(store.folder(folder.id).is_err());
    }
}
