// SPDX-License-Identifier: AGPL-3.0-or-later

//! Notes waiting for a decision after a sync, see [`crate::sync`]'s
//! conflict rules. Every way out keeps the text that is not chosen in the
//! history, nothing is lost whichever button is pressed.

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::{Store, load_note, reindex};
use crate::error::{Error, Result};
use crate::id::NoteId;
use crate::model::VersionReason;
use crate::sync::conflict::ConflictKind;

/// A note's title and body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Side {
    /// Title.
    pub title: String,
    /// Markdown.
    pub body: String,
}

/// What a note asks about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteConflict {
    /// The note.
    pub note_id: NoteId,
    /// `merge` or `editedAfterDelete`.
    pub kind: &'static str,
    /// This device's text before the merge.
    pub mine: Option<Side>,
    /// The other device's text before the merge.
    pub theirs: Option<Side>,
    /// When it was found.
    pub at: i64,
}

/// The way out of a conflict.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", tag = "keep")]
pub enum Resolution {
    /// The merge as it is.
    Merged,
    /// This device's text.
    Mine,
    /// The other device's text.
    Theirs,
    /// This device's text here, the other's in a new note beside it.
    #[serde(rename_all = "camelCase")]
    Both {
        /// Title of the new note.
        copy_title: String,
    },
}

impl Store {
    /// The decision a note waits for, if any.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn note_conflict(&self, note: NoteId) -> Result<Option<NoteConflict>> {
        Ok(self
            .conn
            .query_row(
                "SELECT kind, mine_title, mine_body, theirs_title, theirs_body, at
                 FROM note_conflicts WHERE note_id = ?1",
                [note],
                |row| {
                    let side = |title: Option<String>, body: Option<String>| {
                        title.zip(body).map(|(title, body)| Side { title, body })
                    };
                    Ok(NoteConflict {
                        note_id: note,
                        kind: ConflictKind::parse(&row.get::<_, String>(0)?).as_str(),
                        mine: side(row.get(1)?, row.get(2)?),
                        theirs: side(row.get(3)?, row.get(4)?),
                        at: row.get(5)?,
                    })
                },
            )
            .optional()?)
    }

    /// Notes waiting for a decision, for a mark in lists.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn conflicted_notes(&self) -> Result<Vec<NoteId>> {
        let mut stmt = self
            .conn
            .prepare("SELECT note_id FROM note_conflicts ORDER BY at")?;
        let ids = stmt
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<NoteId>>>()?;
        Ok(ids)
    }

    /// Settles a conflict. The merged text goes to the history before it is
    /// replaced, the text not chosen is already there.
    ///
    /// # Errors
    ///
    /// No conflict for this note, a merge-only choice on a note edited after
    /// deletion, database errors.
    pub fn resolve_conflict(&mut self, note: NoteId, keep: &Resolution) -> Result<()> {
        let conflict = self
            .note_conflict(note)?
            .ok_or(Error::NotFound("conflict"))?;
        let now = self.now();
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM note_conflicts WHERE note_id = ?1", [note])?;
        let chosen = match keep {
            Resolution::Merged => None,
            Resolution::Mine | Resolution::Both { .. } => conflict.mine.clone(),
            Resolution::Theirs => conflict.theirs.clone(),
        };
        if keep != &Resolution::Merged && chosen.is_none() {
            return Err(Error::Invalid("nothing to choose between".into()));
        }
        if let Some(text) = chosen {
            let current = load_note(&tx, note)?;
            super::history_save(
                &tx,
                note,
                &current.title,
                &current.body,
                VersionReason::BeforeRestore,
                now,
            )?;
            tx.execute(
                "UPDATE notes SET title = ?2, body = ?3, updated_at = ?4 WHERE id = ?1",
                params![note, text.title, text.body, now],
            )?;
            reindex(&tx, note)?;
        }
        crate::sync::finish(tx, &mut self.docs, self.clock.now())?;
        if let (Resolution::Both { copy_title }, Some(theirs)) = (keep, conflict.theirs) {
            let original = self.note(note)?;
            self.create_note(&super::NewNote {
                title: copy_title.clone(),
                body: theirs.body,
                folder_id: original.folder_id,
                tags: original.tags,
                pinned: false,
            })?;
        }
        Ok(())
    }
}
