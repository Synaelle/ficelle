// SPDX-License-Identifier: AGPL-3.0-or-later

//! Creating, editing, moving and listing notes.

use rusqlite::params;
use serde::Deserialize;

use super::{
    NOTE_COLUMNS, Store, clean_text, folders, history, links, load_note, reindex, summary_from_row,
    tags,
};
use crate::error::{Error, Result};
use crate::id::{FolderId, NoteId};
use crate::model::{Note, NoteScope, NoteSort, NoteSummary};

/// Fields for a new note. Everything is optional.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NewNote {
    /// Folder, `None` for the top level.
    pub folder_id: Option<FolderId>,
    /// Title.
    pub title: String,
    /// Markdown body.
    pub body: String,
    /// Tag names.
    pub tags: Vec<String>,
    /// Pinned or not.
    pub pinned: bool,
}

/// A content edit. `None` leaves a field as it is.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NoteChange {
    /// New title.
    pub title: Option<String>,
    /// New markdown body.
    pub body: Option<String>,
    /// The title the edit started from. When another device changed the
    /// title since, both edits are merged instead of this one replacing it.
    pub base_title: Option<String>,
    /// The body the edit started from, as [`Self::base_title`].
    pub base_body: Option<String>,
}

/// The new text of a field: `mine` as is, or merged with what another
/// device wrote since `base`.
fn edited(current: &str, mine: Option<String>, base: Option<&String>) -> Result<String> {
    let Some(mine) = mine else {
        return Ok(current.to_owned());
    };
    let mine = clean_text(&mine);
    match base {
        Some(base) if base != current && mine != current => {
            crate::sync::merge_text(base, current, &mine)
        }
        _ => Ok(mine),
    }
}

impl Store {
    /// Creates a note.
    ///
    /// # Errors
    ///
    /// Fails if the folder does not exist or is in the trash.
    pub fn create_note(&mut self, new: &NewNote) -> Result<Note> {
        let now = self.now();
        let id = NoteId::new();
        let tx = self.conn.transaction()?;
        // a note made in a shared folder is shared with it
        let space = match new.folder_id {
            Some(folder) => {
                folders::require_live(&tx, folder)?;
                folders::space_of(&tx, folder)?
            }
            None => String::new(),
        };
        tx.execute(
            "INSERT INTO notes (id, folder_id, title, body, pinned, created_at, updated_at, space_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7)",
            params![
                id,
                new.folder_id,
                clean_text(&new.title),
                clean_text(&new.body),
                new.pinned,
                now,
                space
            ],
        )?;
        tags::replace_note_tags(&tx, id, &new.tags, now)?;
        reindex(&tx, id)?;
        crate::sync::finish(tx, &mut self.docs, self.clock.now())?;
        load_note(&self.conn, id)
    }

    /// Reads one note, including notes in the trash.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`] if it does not exist.
    pub fn note(&self, id: NoteId) -> Result<Note> {
        load_note(&self.conn, id)
    }

    /// Changes the title and or body. The state before an editing session is
    /// kept in the version history.
    ///
    /// # Errors
    ///
    /// Fails if the note does not exist or is in the trash.
    pub fn update_note(&mut self, id: NoteId, change: NoteChange) -> Result<Note> {
        let now = self.now();
        let tx = self.conn.transaction()?;
        let current = load_note(&tx, id)?;
        if current.trashed_at.is_some() {
            return Err(Error::Invalid(
                "restore the note from the trash before editing it".into(),
            ));
        }
        let mine = crate::sync::conflict::Texts {
            title: change
                .title
                .as_deref()
                .map_or_else(|| current.title.clone(), clean_text),
            body: change
                .body
                .as_deref()
                .map_or_else(|| current.body.clone(), clean_text),
        };
        let base = crate::sync::conflict::Texts {
            title: change
                .base_title
                .clone()
                .unwrap_or_else(|| current.title.clone()),
            body: change
                .base_body
                .clone()
                .unwrap_or_else(|| current.body.clone()),
        };
        let title = edited(&current.title, change.title, change.base_title.as_ref())?;
        let body = edited(&current.body, change.body, change.base_body.as_ref())?;
        if title == current.title && body == current.body {
            return Ok(current);
        }
        // another device's edit landed since the editor's text was loaded:
        // the same rule as for changes taken in from a sync
        let theirs = crate::sync::conflict::Texts {
            title: current.title.clone(),
            body: current.body.clone(),
        };
        if theirs != base && crate::sync::conflict::suspicious(&base, &mine, &theirs) {
            crate::sync::conflict::record_merge(&tx, id, &mine, &theirs, now)?;
        }
        history::snapshot_if_new_session(&tx, &current, now)?;
        tx.execute(
            "UPDATE notes SET title = ?2, body = ?3, updated_at = ?4 WHERE id = ?1",
            params![id, title, body, now],
        )?;
        reindex(&tx, id)?;
        links::follow_rename(&tx, id, &current.title, &title, now)?;
        crate::sync::finish(tx, &mut self.docs, self.clock.now())?;
        load_note(&self.conn, id)
    }

    /// Pins or unpins a note.
    ///
    /// # Errors
    ///
    /// Fails if the note does not exist.
    pub fn set_pinned(&mut self, id: NoteId, pinned: bool) -> Result<()> {
        let now = self.now();
        let tx = self.conn.transaction()?;
        let changed = tx.execute(
            "UPDATE notes SET pinned = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, pinned, now],
        )?;
        if changed == 0 {
            return Err(Error::NotFound("note"));
        }
        crate::sync::finish(tx, &mut self.docs, now)
    }

    /// Moves a note to a folder, or to the top level of its space with
    /// `None`. Into a folder of another space it moves to that space and
    /// gets a new id there: returns the note's id after the move.
    ///
    /// # Errors
    ///
    /// Fails if the note or folder does not exist, or the folder is in the trash.
    pub fn move_note(&mut self, id: NoteId, folder: Option<FolderId>) -> Result<NoteId> {
        // into a folder of another space: it moves to that space, with a
        // new id there
        if let Some(target) = folder {
            let to = folders::space_of(&self.conn, target)?;
            if to != load_note(&self.conn, id)?.space_id {
                let moved: NoteId = self
                    .move_to_space(super::Item::Note(id), &to)?
                    .parse()
                    .map_err(|_| Error::Corrupt("moved note id".into()))?;
                return self.move_note(moved, folder);
            }
        }
        let now = self.now();
        let tx = self.conn.transaction()?;
        if let Some(folder) = folder {
            folders::require_live(&tx, folder)?;
        }
        let changed = tx.execute(
            "UPDATE notes SET folder_id = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, folder, now],
        )?;
        if changed == 0 {
            return Err(Error::NotFound("note"));
        }
        crate::sync::finish(tx, &mut self.docs, self.clock.now())?;
        Ok(id)
    }

    /// Lists notes, pinned first, then in the requested order.
    ///
    /// # Errors
    ///
    /// Fails only on database errors.
    pub fn list_notes(&self, scope: NoteScope, sort: NoteSort) -> Result<Vec<NoteSummary>> {
        let order = match sort {
            NoteSort::UpdatedDesc => "updated_at DESC",
            NoteSort::CreatedDesc => "created_at DESC",
            NoteSort::TitleAsc => "title COLLATE NOCASE ASC, updated_at DESC",
        };
        let (filter, value): (&str, Option<String>) = match scope {
            NoteScope::All => ("trashed_at IS NULL", None),
            NoteScope::Folder(None) => ("trashed_at IS NULL AND folder_id IS NULL", None),
            NoteScope::Folder(Some(folder)) => (
                "trashed_at IS NULL AND folder_id = ?1",
                Some(folder.to_string()),
            ),
            NoteScope::Tag(tag) => (
                "trashed_at IS NULL AND id IN (SELECT note_id FROM note_tags WHERE tag_id = ?1)",
                Some(tag.to_string()),
            ),
            NoteScope::Trash => ("trashed_at IS NOT NULL", None),
        };
        let pinned_first = if scope == NoteScope::Trash {
            ""
        } else {
            "pinned DESC, "
        };
        let sql = format!(
            "SELECT {NOTE_COLUMNS} FROM notes WHERE {filter} ORDER BY {pinned_first}{order}"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows: rusqlite::Result<Vec<NoteSummary>> = match value {
            Some(value) => stmt.query_map([value], summary_from_row)?.collect(),
            None => stmt.query_map([], summary_from_row)?.collect(),
        };
        Ok(rows?)
    }
}

#[cfg(test)]
mod tests {
    use crate::clock::MINUTE;
    use crate::model::{NoteScope, NoteSort};
    use crate::store::test_support::store;
    use crate::store::{NewNote, NoteChange};

    fn titled(title: &str) -> NewNote {
        NewNote {
            title: title.into(),
            ..NewNote::default()
        }
    }

    #[test]
    fn creates_and_reads_back_a_note() {
        let (mut store, _) = store();
        let note = store
            .create_note(&NewNote {
                title: "Tomato trellis plan".into(),
                body: "Six cedar posts\u{2}".into(),
                tags: vec!["garden".into(), "#todo".into()],
                ..NewNote::default()
            })
            .unwrap();
        let read = store.note(note.id).unwrap();
        assert_eq!(read.body, "Six cedar posts");
        assert_eq!(read.tags, vec!["garden", "todo"]);
        assert_eq!(read.created_at, read.updated_at);
    }

    #[test]
    fn update_changes_content_and_time() {
        let (mut store, clock) = store();
        let note = store.create_note(&titled("a")).unwrap();
        clock.advance(MINUTE);
        let updated = store
            .update_note(
                note.id,
                NoteChange {
                    body: Some("new body".into()),
                    ..NoteChange::default()
                },
            )
            .unwrap();
        assert_eq!(updated.title, "a");
        assert_eq!(updated.body, "new body");
        assert_eq!(updated.updated_at, note.updated_at + MINUTE);
    }

    #[test]
    fn identical_update_is_a_no_op() {
        let (mut store, clock) = store();
        let note = store.create_note(&titled("a")).unwrap();
        clock.advance(MINUTE);
        let same = store
            .update_note(
                note.id,
                NoteChange {
                    title: Some("a".into()),
                    ..NoteChange::default()
                },
            )
            .unwrap();
        assert_eq!(same.updated_at, note.updated_at);
    }

    #[test]
    fn lists_pinned_first_then_by_sort() {
        let (mut store, clock) = store();
        let a = store.create_note(&titled("Banana")).unwrap();
        clock.advance(MINUTE);
        let b = store.create_note(&titled("apple")).unwrap();
        clock.advance(MINUTE);
        let c = store.create_note(&titled("Cherry")).unwrap();
        store.set_pinned(a.id, true).unwrap();

        let ids = |sort| {
            store
                .list_notes(NoteScope::All, sort)
                .unwrap()
                .into_iter()
                .map(|n| n.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(NoteSort::CreatedDesc), vec![a.id, c.id, b.id]);
        assert_eq!(ids(NoteSort::TitleAsc), vec![a.id, b.id, c.id]);
    }

    #[test]
    fn folder_scope_and_moves() {
        let (mut store, _) = store();
        let folder = store.create_folder(None, "Garden").unwrap();
        let note = store.create_note(&titled("a")).unwrap();
        assert_eq!(
            store
                .list_notes(NoteScope::Folder(None), NoteSort::default())
                .unwrap()
                .len(),
            1
        );
        store.move_note(note.id, Some(folder.id)).unwrap();
        assert!(
            store
                .list_notes(NoteScope::Folder(None), NoteSort::default())
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            store
                .list_notes(NoteScope::Folder(Some(folder.id)), NoteSort::default())
                .unwrap()[0]
                .id,
            note.id
        );
    }

    #[test]
    fn unknown_note_is_not_found() {
        let (mut store, _) = store();
        assert!(store.set_pinned(crate::id::NoteId::new(), true).is_err());
    }
}
