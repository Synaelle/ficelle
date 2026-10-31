// SPDX-License-Identifier: AGPL-3.0-or-later

//! Tags. Names are unique ignoring case and keep the spelling first typed.

use rusqlite::{Connection, OptionalExtension, params};

use super::{Store, clean_text, reindex};
use crate::clock::Timestamp;
use crate::error::{Error, Result};
use crate::id::{NoteId, TagId};
use crate::model::Tag;

/// Turns user input into a tag name: no leading `#`, single spaces, trimmed.
pub(super) fn normalize(name: &str) -> String {
    clean_text(name)
        .trim()
        .trim_start_matches('#')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn note_tag_names(conn: &Connection, note: NoteId) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT t.name FROM tags t JOIN note_tags nt ON nt.tag_id = t.id
         WHERE nt.note_id = ?1 ORDER BY t.name COLLATE NOCASE",
    )?;
    let names = stmt
        .query_map([note], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(names)
}

fn tag_id_for(conn: &Connection, name: &str, now: Timestamp) -> Result<TagId> {
    let existing: Option<TagId> = conn
        .query_row("SELECT id FROM tags WHERE name = ?1", [name], |row| {
            row.get(0)
        })
        .optional()?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let id = TagId::new();
    conn.execute(
        "INSERT INTO tags (id, name, created_at) VALUES (?1, ?2, ?3)",
        params![id, name, now],
    )?;
    Ok(id)
}

/// Replaces the tags of a note and drops tags nobody uses anymore.
pub(crate) fn replace_note_tags(
    conn: &Connection,
    note: NoteId,
    names: &[String],
    now: Timestamp,
) -> Result<()> {
    conn.execute("DELETE FROM note_tags WHERE note_id = ?1", [note])?;
    for name in names.iter().map(|n| normalize(n)).filter(|n| !n.is_empty()) {
        let tag = tag_id_for(conn, &name, now)?;
        conn.execute(
            "INSERT OR IGNORE INTO note_tags (note_id, tag_id) VALUES (?1, ?2)",
            params![note, tag],
        )?;
    }
    conn.execute(
        "DELETE FROM tags WHERE id NOT IN (SELECT tag_id FROM note_tags)",
        [],
    )?;
    Ok(())
}

impl Store {
    /// Replaces the tags of a note. Unknown names create tags, tags left with
    /// no note are removed.
    ///
    /// # Errors
    ///
    /// Fails if the note does not exist.
    pub fn set_note_tags(&mut self, note: NoteId, names: &[String]) -> Result<()> {
        let now = self.now();
        let tx = self.conn.transaction()?;
        let changed = tx.execute(
            "UPDATE notes SET updated_at = ?2 WHERE id = ?1",
            params![note, now],
        )?;
        if changed == 0 {
            return Err(Error::NotFound("note"));
        }
        replace_note_tags(&tx, note, names, now)?;
        reindex(&tx, note)?;
        crate::sync::finish(tx, &mut self.docs, self.clock.now())?;
        Ok(())
    }

    /// Every tag with its count of live notes, by name.
    ///
    /// # Errors
    ///
    /// Fails only on database errors.
    pub fn list_tags(&self) -> Result<Vec<Tag>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.name,
                    (SELECT count(*) FROM note_tags nt JOIN notes n ON n.id = nt.note_id
                     WHERE nt.tag_id = t.id AND n.trashed_at IS NULL)
             FROM tags t ORDER BY t.name COLLATE NOCASE",
        )?;
        let tags = stmt
            .query_map([], |row| {
                Ok(Tag {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    note_count: row.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(tags)
    }

    /// Renames a tag everywhere. Renaming onto an existing tag merges them.
    ///
    /// # Errors
    ///
    /// Fails if the tag does not exist or the new name is empty.
    pub fn rename_tag(&mut self, id: TagId, name: &str) -> Result<()> {
        let name = normalize(name);
        if name.is_empty() {
            return Err(Error::Invalid("a tag needs a name".into()));
        }
        let tx = self.conn.transaction()?;
        let notes = {
            let mut stmt = tx.prepare("SELECT note_id FROM note_tags WHERE tag_id = ?1")?;
            stmt.query_map([id], |row| row.get::<_, NoteId>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let target: Option<TagId> = tx
            .query_row(
                "SELECT id FROM tags WHERE name = ?1 AND id != ?2",
                params![name, id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(target) = target {
            tx.execute(
                "INSERT OR IGNORE INTO note_tags (note_id, tag_id)
                 SELECT note_id, ?2 FROM note_tags WHERE tag_id = ?1",
                params![id, target],
            )?;
            tx.execute("DELETE FROM tags WHERE id = ?1", [id])?;
        } else {
            let changed =
                tx.execute("UPDATE tags SET name = ?2 WHERE id = ?1", params![id, name])?;
            if changed == 0 {
                return Err(Error::NotFound("tag"));
            }
        }
        for note in notes {
            reindex(&tx, note)?;
        }
        crate::sync::finish(tx, &mut self.docs, self.clock.now())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::normalize;
    use crate::store::NewNote;
    use crate::store::test_support::store;

    #[test]
    fn normalizes_names() {
        assert_eq!(normalize("  #seed   trays "), "seed trays");
        assert_eq!(normalize("#"), "");
    }

    #[test]
    fn tags_are_unique_ignoring_case_and_cleaned_up() {
        let (mut store, _) = store();
        let a = store
            .create_note(&NewNote {
                tags: vec!["Garden".into()],
                ..NewNote::default()
            })
            .unwrap();
        let b = store
            .create_note(&NewNote {
                tags: vec!["garden".into(), "todo".into()],
                ..NewNote::default()
            })
            .unwrap();
        let tags = store.list_tags().unwrap();
        assert_eq!(
            tags.iter()
                .map(|t| (t.name.as_str(), t.note_count))
                .collect::<Vec<_>>(),
            vec![("Garden", 2), ("todo", 1)]
        );

        store.set_note_tags(b.id, &[]).unwrap();
        store.set_note_tags(a.id, &[]).unwrap();
        assert!(store.list_tags().unwrap().is_empty());
    }

    #[test]
    fn rename_onto_existing_tag_merges() {
        let (mut store, _) = store();
        let n = store
            .create_note(&NewNote {
                tags: vec!["recipe".into(), "recettes".into()],
                ..NewNote::default()
            })
            .unwrap();
        let recipe = store
            .list_tags()
            .unwrap()
            .into_iter()
            .find(|t| t.name == "recipe")
            .unwrap();
        store.rename_tag(recipe.id, "Recettes").unwrap();
        assert_eq!(store.note(n.id).unwrap().tags, vec!["recettes"]);
        assert_eq!(store.list_tags().unwrap().len(), 1);
    }
}
