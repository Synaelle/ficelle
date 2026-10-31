// SPDX-License-Identifier: AGPL-3.0-or-later

//! Wiki link index: backlinks, link targets, completion, and keeping links
//! right when a note is renamed.

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use super::{NOTE_COLUMNS, Store, history, load_note, reindex, summary_from_row};
use crate::clock::Timestamp;
use crate::error::Result;
use crate::id::NoteId;
use crate::links::{self, title_key};
use crate::model::NoteSummary;

/// A note offered while typing `[[`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkSuggestion {
    /// Note id.
    pub id: NoteId,
    /// Its title, to insert.
    pub title: String,
}

/// Stores the title key and the distinct link targets of one note.
pub(super) fn index_note(conn: &Connection, id: NoteId, title: &str, body: &str) -> Result<()> {
    conn.execute(
        "UPDATE notes SET title_key = ?2 WHERE id = ?1",
        params![id, title_key(title)],
    )?;
    conn.execute("DELETE FROM note_links WHERE source_id = ?1", [id])?;
    for link in links::parse(body) {
        conn.execute(
            "INSERT OR IGNORE INTO note_links (source_id, target_key) VALUES (?1, ?2)",
            params![id, title_key(&link.target)],
        )?;
    }
    Ok(())
}

/// Indexes every note, used once when the link index is created.
pub(crate) fn rebuild_link_index(conn: &Connection) -> Result<()> {
    let notes = {
        let mut stmt = conn.prepare("SELECT id, title, body FROM notes")?;
        stmt.query_map([], |row| {
            Ok((
                row.get::<_, NoteId>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (id, title, body) in notes {
        index_note(conn, id, &title, &body)?;
    }
    Ok(())
}

/// After a rename, points links that used the old title at the new one.
/// Skipped when another live note still has the old title, because the
/// links may well mean that note.
pub(super) fn follow_rename(
    conn: &Connection,
    renamed: NoteId,
    old: &str,
    new: &str,
    now: Timestamp,
) -> Result<()> {
    let old_key = title_key(old);
    if old_key.is_empty() || old_key == title_key(new) {
        return Ok(());
    }
    let still_used: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM notes WHERE title_key = ?1 AND id != ?2 AND trashed_at IS NULL)",
        params![old_key, renamed],
        |row| row.get(0),
    )?;
    if still_used {
        return Ok(());
    }
    let sources = {
        let mut stmt = conn.prepare("SELECT source_id FROM note_links WHERE target_key = ?1")?;
        stmt.query_map([&old_key], |row| row.get::<_, NoteId>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for source in sources {
        let note = load_note(conn, source)?;
        if let Some(body) = links::retarget(&note.body, old, new) {
            history::snapshot_if_new_session(conn, &note, now)?;
            conn.execute(
                "UPDATE notes SET body = ?2, updated_at = ?3 WHERE id = ?1",
                params![source, body, now],
            )?;
            reindex(conn, source)?;
        }
    }
    Ok(())
}

/// Escapes `%`, `_` and `\` for a LIKE pattern using `\` as escape.
fn like_escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

impl Store {
    /// Live notes that link to this note by its current title.
    ///
    /// # Errors
    ///
    /// Fails if the note does not exist.
    pub fn backlinks(&self, note: NoteId) -> Result<Vec<NoteSummary>> {
        let key = title_key(&self.note(note)?.title);
        if key.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {NOTE_COLUMNS} FROM notes
             WHERE trashed_at IS NULL AND id != ?2
               AND id IN (SELECT source_id FROM note_links WHERE target_key = ?1)
             ORDER BY updated_at DESC"
        ))?;
        let rows = stmt
            .query_map(params![key, note], summary_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// The live note a link title points to. With several notes of the same
    /// title, the most recently changed wins.
    ///
    /// # Errors
    ///
    /// Fails only on database errors.
    pub fn resolve_link(&self, target: &str) -> Result<Option<NoteId>> {
        use rusqlite::OptionalExtension;
        Ok(self
            .conn
            .query_row(
                "SELECT id FROM notes WHERE title_key = ?1 AND trashed_at IS NULL
                 ORDER BY updated_at DESC LIMIT 1",
                [title_key(target)],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Notes to offer while typing a link: titles starting with the input
    /// first, then titles containing it, most recent first in each group.
    ///
    /// # Errors
    ///
    /// Fails only on database errors.
    pub fn link_suggestions(&self, input: &str, limit: usize) -> Result<Vec<LinkSuggestion>> {
        let key = like_escape(&title_key(input));
        let mut stmt = self.conn.prepare(
            "SELECT id, title FROM notes
             WHERE trashed_at IS NULL AND title != '' AND title_key LIKE '%' || ?1 || '%' ESCAPE '\\'
             ORDER BY (title_key LIKE ?1 || '%' ESCAPE '\\') DESC, updated_at DESC
             LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(
                params![key, i64::try_from(limit).unwrap_or(i64::MAX)],
                |row| {
                    Ok(LinkSuggestion {
                        id: row.get(0)?,
                        title: row.get(1)?,
                    })
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Headings of a note, for `[[Note#` completion.
    ///
    /// # Errors
    ///
    /// Fails if the note does not exist.
    pub fn note_headings(&self, note: NoteId) -> Result<Vec<String>> {
        Ok(links::headings(&self.note(note)?.body))
    }
}

#[cfg(test)]
mod tests {
    use crate::clock::MINUTE;
    use crate::store::test_support::store;
    use crate::store::{NewNote, NoteChange};

    fn note(title: &str, body: &str) -> NewNote {
        NewNote {
            title: title.into(),
            body: body.into(),
            ..NewNote::default()
        }
    }

    #[test]
    fn backlinks_follow_titles_ignoring_case_and_code() {
        let (mut store, _) = store();
        let target = store.create_note(&note("Garden log", "")).unwrap();
        let a = store.create_note(&note("A", "see [[garden LOG]]")).unwrap();
        store
            .create_note(&note("B", "`[[Garden log]]` is code"))
            .unwrap();
        let c = store
            .create_note(&note("C", "[[Garden log#2025|last year]]"))
            .unwrap();
        let ids: Vec<_> = store
            .backlinks(target.id)
            .unwrap()
            .into_iter()
            .map(|n| n.id)
            .collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&a.id) && ids.contains(&c.id));

        store.trash_note(a.id).unwrap();
        assert_eq!(store.backlinks(target.id).unwrap().len(), 1);
    }

    #[test]
    fn resolves_accented_titles_without_case() {
        let (mut store, _) = store();
        let n = store.create_note(&note("Call with Inès", "")).unwrap();
        assert_eq!(store.resolve_link("call with INÈS").unwrap(), Some(n.id));
        assert_eq!(store.resolve_link("nobody").unwrap(), None);
    }

    #[test]
    fn suggestions_put_prefix_matches_first_and_escape_like() {
        let (mut store, clock) = store();
        store.create_note(&note("Plan for tomatoes", "")).unwrap();
        clock.advance(MINUTE);
        store.create_note(&note("Tomato trellis", "")).unwrap();
        clock.advance(MINUTE);
        store.create_note(&note("100% hydration", "")).unwrap();
        let titles: Vec<_> = store
            .link_suggestions("tomat", 10)
            .unwrap()
            .into_iter()
            .map(|s| s.title)
            .collect();
        assert_eq!(titles, vec!["Tomato trellis", "Plan for tomatoes"]);
        assert_eq!(store.link_suggestions("0%", 10).unwrap().len(), 1);
        assert_eq!(store.link_suggestions("_", 10).unwrap().len(), 0);
    }

    #[test]
    fn renaming_updates_links_elsewhere() {
        let (mut store, clock) = store();
        let target = store.create_note(&note("Old name", "")).unwrap();
        let linker = store
            .create_note(&note(
                "Linker",
                "[[Old name]] and [[old name#Part|label]] and `[[Old name]]`",
            ))
            .unwrap();
        clock.advance(MINUTE);
        store
            .update_note(
                target.id,
                NoteChange {
                    title: Some("New name".into()),
                    ..NoteChange::default()
                },
            )
            .unwrap();
        assert_eq!(
            store.note(linker.id).unwrap().body,
            "[[New name]] and [[New name#Part|label]] and `[[Old name]]`"
        );
        assert_eq!(store.backlinks(target.id).unwrap().len(), 1);
        assert_eq!(
            store.list_versions(linker.id).unwrap()[0].body,
            "[[Old name]] and [[old name#Part|label]] and `[[Old name]]`",
            "the linking note keeps its pre rename text in history"
        );
    }

    #[test]
    fn rename_leaves_links_alone_when_the_old_title_is_still_taken() {
        let (mut store, _) = store();
        let first = store.create_note(&note("Plan", "")).unwrap();
        store.create_note(&note("Plan", "")).unwrap();
        let linker = store.create_note(&note("L", "[[Plan]]")).unwrap();
        store
            .update_note(
                first.id,
                NoteChange {
                    title: Some("Other".into()),
                    ..NoteChange::default()
                },
            )
            .unwrap();
        assert_eq!(store.note(linker.id).unwrap().body, "[[Plan]]");
    }

    #[test]
    fn headings_for_completion() {
        let (mut store, _) = store();
        let n = store
            .create_note(&note("x", "# One\ntext\n## Two"))
            .unwrap();
        assert_eq!(store.note_headings(n.id).unwrap(), vec!["One", "Two"]);
    }
}
