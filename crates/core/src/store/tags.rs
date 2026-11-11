// SPDX-License-Identifier: AGPL-3.0-or-later

//! Tags. A tag can hold tags: its name is a path ("Projects/Garden") and
//! every level has a row of its own. Names are unique ignoring case and keep
//! the spelling first typed.

use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension, params};

use super::{Store, clean_text, reindex};
use crate::clock::Timestamp;
use crate::error::{Error, Result};
use crate::id::{FolderId, NoteId, TagId};
use crate::model::Tag;

/// One level of a tag: no leading `#`, single spaces, trimmed.
fn part(name: &str) -> String {
    name.trim()
        .trim_start_matches('#')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Turns user input into a tag name: its levels joined by `/`, each cleaned
/// like [`part`], empty levels dropped.
pub(super) fn normalize(name: &str) -> String {
    clean_text(name)
        .split('/')
        .map(part)
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

/// What makes two names the same tag. SQLite's NOCASE folds only a to z: É
/// and é would be two tags.
pub(crate) fn key(name: &str) -> String {
    name.to_lowercase()
}

/// The tags above a path, outermost first: `a/b/c` gives `a` and `a/b`.
fn parents(name: &str) -> impl Iterator<Item = &str> {
    name.match_indices('/').map(move |(at, _)| &name[..at])
}

/// Whether `name` is `under` itself or one of the tags it holds.
pub(crate) fn within(name: &str, under: &str) -> bool {
    let (name, under) = (key(name), key(under));
    name == under || name.starts_with(&format!("{under}/"))
}

fn all_tags(conn: &Connection) -> Result<Vec<(TagId, String)>> {
    let mut stmt = conn.prepare("SELECT id, name FROM tags")?;
    let tags = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(tags)
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

/// The tag with this name, made if missing, with every tag above it.
pub(crate) fn tag_id_for(conn: &Connection, name: &str, now: Timestamp) -> Result<TagId> {
    let existing: HashMap<String, TagId> = all_tags(conn)?
        .into_iter()
        .map(|(id, name)| (key(&name), id))
        .collect();
    let mut id = None;
    for level in parents(name).chain(std::iter::once(name)) {
        id = Some(if let Some(&found) = existing.get(&key(level)) {
            found
        } else {
            let made = TagId::new();
            conn.execute(
                "INSERT INTO tags (id, name, created_at) VALUES (?1, ?2, ?3)",
                params![made, level, now],
            )?;
            made
        });
    }
    id.ok_or_else(|| Error::Invalid("a tag needs a name".into()))
}

/// Removes the tags nothing keeps: no note, not made on their own, no colour,
/// and holding no tag that is kept.
pub(crate) fn drop_unused_tags(conn: &Connection) -> Result<()> {
    let tags: Vec<(TagId, String, bool)> = {
        let mut stmt = conn.prepare(
            "SELECT id, name, explicit = 1 OR color IS NOT NULL
                 OR EXISTS (SELECT 1 FROM note_tags nt WHERE nt.tag_id = tags.id)
             FROM tags",
        )?;
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut kept = HashSet::new();
    for (_, name, used) in &tags {
        if *used {
            kept.extend(parents(name).map(key));
            kept.insert(key(name));
        }
    }
    for (id, name, _) in tags {
        if !kept.contains(&key(&name)) {
            conn.execute("DELETE FROM tags WHERE id = ?1", [id])?;
        }
    }
    Ok(())
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
    drop_unused_tags(conn)
}

/// Takes notes out of the folder an older device put them in once they no
/// longer have that folder's tag, so the folder does not bring it back.
pub(crate) fn release_folders(conn: &Connection, notes: &[NoteId]) -> Result<()> {
    let paths = super::folders::paths(conn)?;
    for &note in notes {
        let folder: Option<FolderId> = conn
            .query_row("SELECT folder_id FROM notes WHERE id = ?1", [note], |row| {
                row.get(0)
            })
            .optional()?
            .flatten();
        let Some(path) = folder.and_then(|f| paths.get(&f)) else {
            continue;
        };
        let tagged = note_tag_names(conn, note)?
            .iter()
            .any(|t| key(t) == key(path));
        if !tagged {
            conn.execute("UPDATE notes SET folder_id = NULL WHERE id = ?1", [note])?;
        }
    }
    Ok(())
}

/// A tag and every tag it holds.
fn subtree(conn: &Connection, id: TagId) -> Result<Vec<(TagId, String)>> {
    let tags = all_tags(conn)?;
    let name = tags
        .iter()
        .find(|(t, _)| *t == id)
        .map(|(_, name)| name.clone())
        .ok_or(Error::NotFound("tag"))?;
    Ok(tags.into_iter().filter(|(_, n)| within(n, &name)).collect())
}

fn notes_of(conn: &Connection, tags: &[TagId]) -> Result<Vec<NoteId>> {
    let mut stmt = conn.prepare("SELECT DISTINCT note_id FROM note_tags WHERE tag_id = ?1")?;
    let mut notes = Vec::new();
    for tag in tags {
        for note in stmt.query_map([tag], |row| row.get::<_, NoteId>(0))? {
            let note = note?;
            if !notes.contains(&note) {
                notes.push(note);
            }
        }
    }
    Ok(notes)
}

/// The tags a list shows for a tag: itself and every tag it holds.
pub(super) fn ids_within(conn: &Connection, id: TagId) -> Result<Vec<TagId>> {
    Ok(subtree(conn, id)
        .or_else(|e| match e {
            Error::NotFound(_) => Ok(Vec::new()),
            e => Err(e),
        })?
        .into_iter()
        .map(|(id, _)| id)
        .collect())
}

/// Each folder's notes get its path as a tag, a folder not in the trash
/// stays as a tag made on its own. The search entries of the notes are left
/// to the caller.
pub(crate) fn tags_from_folders(
    conn: &Connection,
    folders: &[FolderId],
    now: Timestamp,
) -> Result<()> {
    let paths = super::folders::paths(conn)?;
    for &folder in folders {
        let Some(path) = paths.get(&folder) else {
            continue;
        };
        let tag = tag_id_for(conn, path, now)?;
        conn.execute(
            "UPDATE tags SET origin_folder = coalesce(origin_folder, ?2),
                 explicit = max(explicit, (SELECT trashed_at IS NULL FROM folders WHERE id = ?2))
             WHERE id = ?1",
            params![tag, folder],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO note_tags (note_id, tag_id)
             SELECT id, ?2 FROM notes WHERE folder_id = ?1",
            params![folder, tag],
        )?;
    }
    drop_unused_tags(conn)
}

/// Folders become tags, once, when a vault takes the nested tags: each
/// folder's notes get its path as a tag, folders with no note stay as tags
/// made on their own. `folder_id` stays set, the documents keep telling older
/// devices where each note is.
pub(crate) fn convert_folders_to_tags(conn: &Connection) -> Result<()> {
    let now = crate::clock::Clock::now(&crate::clock::SystemClock);
    // tags importers made with a "/" in their name are nested from now on,
    // and two that now mean the same tag become one
    for (id, name) in all_tags(conn)? {
        let clean = normalize(&name);
        if clean.is_empty() || clean == name {
            continue;
        }
        let target = tag_id_for(conn, &clean, now)?;
        if target != id {
            conn.execute(
                "INSERT OR IGNORE INTO note_tags (note_id, tag_id)
                 SELECT note_id, ?2 FROM note_tags WHERE tag_id = ?1",
                params![id, target],
            )?;
            conn.execute("DELETE FROM tags WHERE id = ?1", [id])?;
        }
    }
    let folders: Vec<FolderId> = {
        let mut stmt = conn.prepare("SELECT id FROM folders")?;
        stmt.query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    tags_from_folders(conn, &folders, now)?;
    super::rebuild_search_index(conn)?;
    conn.execute(
        "INSERT OR IGNORE INTO sync_touched (kind, space, id) SELECT 'note', space_id, id FROM notes",
        [],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO sync_touched (kind, space, id) VALUES ('tags', '', '*')",
        [],
    )?;
    crate::sync::flush(conn, &mut crate::sync::Docs::default(), now)
}

/// Colour marks a tag may have, the interface draws them muted.
pub const TAG_COLORS: &[&str] = &[
    "red", "orange", "yellow", "green", "teal", "blue", "purple", "grey",
];

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
        release_folders(&tx, &[note])?;
        reindex(&tx, note)?;
        crate::sync::finish(tx, &mut self.docs, self.clock.now())?;
        Ok(())
    }

    /// Every tag by name, with its colour and its count of live notes,
    /// counting those of the tags it holds.
    ///
    /// # Errors
    ///
    /// Fails only on database errors.
    pub fn list_tags(&self) -> Result<Vec<Tag>> {
        let tags: Vec<(TagId, String, Option<String>)> = {
            let mut stmt = self
                .conn
                .prepare("SELECT id, name, color FROM tags ORDER BY name COLLATE NOCASE")?;
            stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let tagged: Vec<(NoteId, String)> = {
            let mut stmt = self.conn.prepare(
                "SELECT nt.note_id, t.name FROM note_tags nt
                 JOIN tags t ON t.id = nt.tag_id JOIN notes n ON n.id = nt.note_id
                 WHERE n.trashed_at IS NULL",
            )?;
            stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        Ok(tags
            .into_iter()
            .map(|(id, name, color)| {
                let notes: HashSet<NoteId> = tagged
                    .iter()
                    .filter(|(_, t)| within(t, &name))
                    .map(|(n, _)| *n)
                    .collect();
                Tag {
                    id,
                    note_count: i64::try_from(notes.len()).unwrap_or(i64::MAX),
                    name,
                    color,
                }
            })
            .collect())
    }

    /// Makes a tag that stays without any note, with every tag above it.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] for an empty name.
    pub fn create_tag(&mut self, name: &str) -> Result<TagId> {
        let name = normalize(name);
        if name.is_empty() {
            return Err(Error::Invalid("a tag needs a name".into()));
        }
        let now = self.now();
        let tx = self.conn.transaction()?;
        let id = tag_id_for(&tx, &name, now)?;
        tx.execute("UPDATE tags SET explicit = 1 WHERE id = ?1", [id])?;
        crate::sync::finish(tx, &mut self.docs, self.clock.now())?;
        Ok(id)
    }

    /// Sets or clears a tag's colour mark, one of [`TAG_COLORS`].
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] for an unknown colour, [`Error::NotFound`] for a
    /// missing tag.
    pub fn set_tag_color(&mut self, id: TagId, color: Option<&str>) -> Result<()> {
        if color.is_some_and(|c| !TAG_COLORS.contains(&c)) {
            return Err(Error::Invalid("unknown tag colour".into()));
        }
        let tx = self.conn.transaction()?;
        if tx.execute(
            "UPDATE tags SET color = ?2 WHERE id = ?1",
            params![id, color],
        )? == 0
        {
            return Err(Error::NotFound("tag"));
        }
        drop_unused_tags(&tx)?;
        crate::sync::finish(tx, &mut self.docs, self.clock.now())?;
        Ok(())
    }

    /// Renames a tag, or moves it under another with a path, taking the tags
    /// it holds along. Onto an existing tag the two merge.
    ///
    /// # Errors
    ///
    /// Fails if the tag does not exist, the new name is empty, or it would
    /// go inside itself.
    pub fn rename_tag(&mut self, id: TagId, name: &str) -> Result<()> {
        let name = normalize(name);
        if name.is_empty() {
            return Err(Error::Invalid("a tag needs a name".into()));
        }
        let now = self.now();
        let tx = self.conn.transaction()?;
        let moved = subtree(&tx, id)?;
        let old = moved
            .iter()
            .find(|(t, _)| *t == id)
            .map(|(_, n)| n.clone())
            .ok_or(Error::NotFound("tag"))?;
        if key(&name) != key(&old) && within(&name, &old) {
            return Err(Error::Invalid("a tag cannot go inside itself".into()));
        }
        let notes = notes_of(&tx, &moved.iter().map(|(t, _)| *t).collect::<Vec<_>>())?;
        for (tag, from) in &moved {
            let rest: String = from.chars().skip(old.chars().count()).collect();
            let to = format!("{name}{rest}");
            let target = all_tags(&tx)?
                .into_iter()
                .find(|(t, n)| t != tag && key(n) == key(&to))
                .map(|(t, _)| t);
            match target {
                Some(target) => {
                    tx.execute(
                        "INSERT OR IGNORE INTO note_tags (note_id, tag_id)
                         SELECT note_id, ?2 FROM note_tags WHERE tag_id = ?1",
                        params![tag, target],
                    )?;
                    tx.execute(
                        "UPDATE tags SET explicit = max(explicit, (SELECT explicit FROM tags WHERE id = ?1)),
                             color = coalesce(color, (SELECT color FROM tags WHERE id = ?1))
                         WHERE id = ?2",
                        params![tag, target],
                    )?;
                    tx.execute("DELETE FROM tags WHERE id = ?1", [tag])?;
                }
                None => {
                    tx.execute("UPDATE tags SET name = ?2 WHERE id = ?1", params![tag, to])?;
                }
            }
        }
        // a tag moved under a new parent needs that parent's row
        tag_id_for(&tx, &name, now)?;
        drop_unused_tags(&tx)?;
        release_folders(&tx, &notes)?;
        for note in notes {
            reindex(&tx, note)?;
        }
        crate::sync::finish(tx, &mut self.docs, self.clock.now())?;
        Ok(())
    }

    /// Removes a tag and the tags it holds from every note. The notes stay.
    ///
    /// # Errors
    ///
    /// Fails if the tag does not exist.
    pub fn delete_tag(&mut self, id: TagId) -> Result<()> {
        let tx = self.conn.transaction()?;
        let gone = subtree(&tx, id)?;
        let ids: Vec<TagId> = gone.iter().map(|(t, _)| *t).collect();
        let notes = notes_of(&tx, &ids)?;
        for tag in &ids {
            tx.execute("DELETE FROM note_tags WHERE tag_id = ?1", [tag])?;
            tx.execute("DELETE FROM tags WHERE id = ?1", [tag])?;
        }
        drop_unused_tags(&tx)?;
        release_folders(&tx, &notes)?;
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
    use crate::model::{NoteScope, NoteSort};
    use crate::store::NewNote;
    use crate::store::test_support::store;

    fn tagged(store: &mut crate::store::Store, tags: &[&str]) -> crate::id::NoteId {
        store
            .create_note(&NewNote {
                tags: tags.iter().map(|t| (*t).to_string()).collect(),
                ..NewNote::default()
            })
            .unwrap()
            .id
    }

    fn tag(store: &crate::store::Store, name: &str) -> crate::model::Tag {
        store
            .list_tags()
            .unwrap()
            .into_iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("no tag {name}"))
    }

    fn names(store: &crate::store::Store) -> Vec<(String, i64)> {
        store
            .list_tags()
            .unwrap()
            .into_iter()
            .map(|t| (t.name, t.note_count))
            .collect()
    }

    #[test]
    fn tags_differing_only_by_case_are_one_tag_accents_too() {
        let (mut store, _) = store();
        let note = store.create_note(&NewNote::default()).unwrap();
        store
            .set_note_tags(
                note.id,
                &["été".into(), "ÉTÉ".into(), "Work".into(), "work".into()],
            )
            .unwrap();
        assert_eq!(store.note(note.id).unwrap().tags, ["Work", "été"]);
        let other = store.create_note(&NewNote::default()).unwrap();
        store.set_note_tags(other.id, &["ÉTÉ".into()]).unwrap();
        assert_eq!(store.note(other.id).unwrap().tags, ["été"]);
        assert_eq!(store.list_tags().unwrap().len(), 2);
    }

    #[test]
    fn normalizes_names() {
        assert_eq!(normalize("  #seed   trays "), "seed trays");
        assert_eq!(normalize("#"), "");
        assert_eq!(
            normalize(" Projects / #Garden// beds "),
            "Projects/Garden/beds"
        );
        assert_eq!(normalize("/"), "");
    }

    #[test]
    fn a_tag_holds_tags_and_counts_their_notes() {
        let (mut store, _) = store();
        let beans = tagged(&mut store, &["Projects/Garden"]);
        let plan = tagged(&mut store, &["projects"]);
        let loose = tagged(&mut store, &[]);
        assert_eq!(
            names(&store),
            [("Projects".into(), 2), ("Projects/Garden".into(), 1)]
        );
        let projects = tag(&store, "Projects");
        let listed: Vec<_> = store
            .list_notes(NoteScope::Tag(projects.id), NoteSort::TitleAsc)
            .unwrap()
            .into_iter()
            .map(|n| n.id)
            .collect();
        assert!(listed.contains(&beans) && listed.contains(&plan) && listed.len() == 2);
        let untagged: Vec<_> = store
            .list_notes(NoteScope::Untagged, NoteSort::TitleAsc)
            .unwrap()
            .into_iter()
            .map(|n| n.id)
            .collect();
        assert_eq!(untagged, [loose]);
        // the parent goes with its last child
        store.set_note_tags(beans, &[]).unwrap();
        store.set_note_tags(plan, &[]).unwrap();
        assert!(names(&store).is_empty());
    }

    #[test]
    fn a_tag_made_alone_or_coloured_stays_without_notes() {
        let (mut store, _) = store();
        store.create_tag("Ideas/Later").unwrap();
        assert_eq!(
            names(&store),
            [("Ideas".into(), 0), ("Ideas/Later".into(), 0)]
        );
        let n = tagged(&mut store, &["Reading"]);
        let reading = tag(&store, "Reading");
        store.set_tag_color(reading.id, Some("teal")).unwrap();
        store.set_note_tags(n, &[]).unwrap();
        assert_eq!(tag(&store, "Reading").color.as_deref(), Some("teal"));
        assert!(store.set_tag_color(reading.id, Some("neon")).is_err());
        store.set_tag_color(reading.id, None).unwrap();
        assert!(names(&store).iter().all(|(n, _)| n != "Reading"));
    }

    #[test]
    fn moving_a_tag_takes_what_it_holds_and_merges_onto_a_tag_there() {
        let (mut store, _) = store();
        let beans = tagged(&mut store, &["Garden/Beans"]);
        let peas = tagged(&mut store, &["Projects/Garden/Peas"]);
        let garden = tag(&store, "Garden");
        assert!(store.rename_tag(garden.id, "Garden/Inside").is_err());
        store.rename_tag(garden.id, "Projects/Garden").unwrap();
        assert_eq!(store.note(beans).unwrap().tags, ["Projects/Garden/Beans"]);
        assert_eq!(store.note(peas).unwrap().tags, ["Projects/Garden/Peas"]);
        assert_eq!(
            names(&store),
            [
                ("Projects".into(), 2),
                ("Projects/Garden".into(), 2),
                ("Projects/Garden/Beans".into(), 1),
                ("Projects/Garden/Peas".into(), 1)
            ]
        );
    }

    #[test]
    fn deleting_a_tag_takes_it_and_what_it_holds_off_every_note() {
        let (mut store, _) = store();
        let a = tagged(&mut store, &["Work/Q1", "Home"]);
        let b = tagged(&mut store, &["work"]);
        store.delete_tag(tag(&store, "Work").id).unwrap();
        assert_eq!(store.note(a).unwrap().tags, ["Home"]);
        assert!(store.note(b).unwrap().tags.is_empty());
        assert_eq!(names(&store), [("Home".into(), 1)]);
    }

    #[test]
    fn a_note_leaves_its_old_folder_with_the_folders_tag() {
        let (mut store, _) = store();
        let folder = store.create_folder(None, "Inbox").unwrap();
        let n = tagged(&mut store, &["Inbox"]);
        store.move_note(n, Some(folder.id)).unwrap();
        store
            .set_note_tags(n, &["Inbox".into(), "Later".into()])
            .unwrap();
        assert_eq!(store.note(n).unwrap().folder_id, Some(folder.id));
        store.set_note_tags(n, &["Later".into()]).unwrap();
        assert_eq!(store.note(n).unwrap().folder_id, None);
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
