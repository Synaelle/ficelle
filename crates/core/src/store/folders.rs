// SPDX-License-Identifier: AGPL-3.0-or-later

//! Nested folders.

use rusqlite::{Connection, OptionalExtension, Row, params};

use super::{Store, clean_text};
use crate::error::{Error, Result};
use crate::id::FolderId;
use crate::model::Folder;

const FOLDER_COLUMNS: &str =
    "id, parent_id, name, position, created_at, updated_at, trashed_at, space_id";

fn folder_from_row(row: &Row<'_>) -> rusqlite::Result<Folder> {
    Ok(Folder {
        id: row.get(0)?,
        parent_id: row.get(1)?,
        name: row.get(2)?,
        position: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
        trashed_at: row.get(6)?,
        space_id: row.get(7)?,
    })
}

pub(super) fn load(conn: &Connection, id: FolderId) -> Result<Folder> {
    conn.query_row(
        &format!("SELECT {FOLDER_COLUMNS} FROM folders WHERE id = ?1"),
        [id],
        folder_from_row,
    )
    .optional()?
    .ok_or(Error::NotFound("folder"))
}

/// Fails unless the folder exists and is not in the trash.
pub(super) fn require_live(conn: &Connection, id: FolderId) -> Result<()> {
    if load(conn, id)?.trashed_at.is_some() {
        return Err(Error::Invalid("this folder is in the trash".into()));
    }
    Ok(())
}

/// The space a folder belongs to.
pub(super) fn space_of(conn: &Connection, id: FolderId) -> Result<String> {
    conn.query_row("SELECT space_id FROM folders WHERE id = ?1", [id], |row| {
        row.get(0)
    })
    .optional()?
    .ok_or(Error::NotFound("folder"))
}

/// The folder and every folder below it.
pub(super) fn subtree(conn: &Connection, root: FolderId) -> Result<Vec<FolderId>> {
    let mut stmt = conn.prepare(
        "WITH RECURSIVE tree(id) AS (
             SELECT ?1
             UNION ALL
             SELECT f.id FROM folders f JOIN tree t ON f.parent_id = t.id
         )
         SELECT id FROM tree",
    )?;
    let ids = stmt
        .query_map([root], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(ids)
}

pub(super) fn clean_name(name: &str) -> Result<String> {
    let name = clean_text(name)
        .replace(['\n', '\r', '\t'], " ")
        .trim()
        .to_owned();
    if name.is_empty() {
        return Err(Error::Invalid("a folder needs a name".into()));
    }
    Ok(name)
}

impl Store {
    /// Creates a folder at the end of its parent.
    ///
    /// # Errors
    ///
    /// Fails if the name is empty or the parent is missing or in the trash.
    pub fn create_folder(&mut self, parent: Option<FolderId>, name: &str) -> Result<Folder> {
        let name = clean_name(name)?;
        let now = self.now();
        let id = FolderId::new();
        let tx = self.conn.transaction()?;
        // a folder made inside a shared folder is shared with it
        let space = match parent {
            Some(parent) => {
                require_live(&tx, parent)?;
                space_of(&tx, parent)?
            }
            None => String::new(),
        };
        let position: i64 = tx.query_row(
            "SELECT coalesce(max(position) + 1, 0) FROM folders WHERE parent_id IS ?1",
            [parent],
            |row| row.get(0),
        )?;
        tx.execute(
            "INSERT INTO folders (id, parent_id, name, position, created_at, updated_at, space_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)",
            params![id, parent, name, position, now, space],
        )?;
        crate::sync::finish(tx, &mut self.docs, self.clock.now())?;
        load(&self.conn, id)
    }

    /// Reads one folder.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`] if it does not exist.
    pub fn folder(&self, id: FolderId) -> Result<Folder> {
        load(&self.conn, id)
    }

    /// Every folder not in the trash, ordered by parent then position. The ui
    /// builds the tree from `parent_id`.
    ///
    /// # Errors
    ///
    /// Fails only on database errors.
    pub fn list_folders(&self) -> Result<Vec<Folder>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {FOLDER_COLUMNS} FROM folders WHERE trashed_at IS NULL
             ORDER BY parent_id, position, name COLLATE NOCASE"
        ))?;
        let folders = stmt
            .query_map([], folder_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(folders)
    }

    /// Renames a folder.
    ///
    /// # Errors
    ///
    /// Fails if the name is empty or the folder does not exist.
    pub fn rename_folder(&mut self, id: FolderId, name: &str) -> Result<()> {
        let name = clean_name(name)?;
        let now = self.now();
        // in one transaction with its record: refused there (shared to
        // read only), the row is not changed either
        let tx = self.conn.transaction()?;
        let changed = tx.execute(
            "UPDATE folders SET name = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, name, now],
        )?;
        if changed == 0 {
            return Err(Error::NotFound("folder"));
        }
        crate::sync::finish(tx, &mut self.docs, now)
    }

    /// Moves a folder under another one, or to the top level with `None`.
    /// It goes to the end of its new parent. Into a folder of another
    /// space it moves to that space with a new id: returns the folder's id
    /// after the move.
    ///
    /// # Errors
    ///
    /// Fails when the move would put a folder inside itself, or the target is
    /// missing or in the trash.
    pub fn move_folder(&mut self, id: FolderId, parent: Option<FolderId>) -> Result<FolderId> {
        // into a folder of another space: it moves to that space, with a
        // new id there
        if let Some(target) = parent {
            let to = space_of(&self.conn, target)?;
            if to != space_of(&self.conn, id)? {
                let moved: FolderId = self
                    .move_to_space(super::Item::Folder(id), &to)?
                    .parse()
                    .map_err(|_| Error::Corrupt("moved folder id".into()))?;
                return self.move_folder(moved, parent);
            }
        }
        let now = self.now();
        let tx = self.conn.transaction()?;
        load(&tx, id)?;
        if let Some(parent) = parent {
            require_live(&tx, parent)?;
            if subtree(&tx, id)?.contains(&parent) {
                return Err(Error::Invalid(
                    "a folder cannot be moved inside itself".into(),
                ));
            }
        }
        let position: i64 = tx.query_row(
            "SELECT coalesce(max(position) + 1, 0) FROM folders WHERE parent_id IS ?1",
            [parent],
            |row| row.get(0),
        )?;
        tx.execute(
            "UPDATE folders SET parent_id = ?2, position = ?3, updated_at = ?4 WHERE id = ?1",
            params![id, parent, position, now],
        )?;
        crate::sync::finish(tx, &mut self.docs, self.clock.now())?;
        Ok(id)
    }

    /// Sets the order of a parent's children. Folders not listed keep their
    /// place after the listed ones.
    ///
    /// # Errors
    ///
    /// Fails if a listed folder does not belong to `parent`.
    pub fn reorder_folders(&mut self, parent: Option<FolderId>, order: &[FolderId]) -> Result<()> {
        let tx = self.conn.transaction()?;
        for (position, id) in order.iter().enumerate() {
            let changed = tx.execute(
                "UPDATE folders SET position = ?3 WHERE id = ?1 AND parent_id IS ?2",
                params![id, parent, i64::try_from(position).unwrap_or(i64::MAX)],
            )?;
            if changed == 0 {
                return Err(Error::Invalid(
                    "a folder in the new order is not in this parent".into(),
                ));
            }
        }
        crate::sync::finish(tx, &mut self.docs, self.clock.now())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::store::test_support::store;

    #[test]
    fn creates_nested_folders_in_order() {
        let (mut store, _) = store();
        let projects = store.create_folder(None, "Projects").unwrap();
        let garden = store.create_folder(Some(projects.id), "Garden").unwrap();
        let kitchen = store
            .create_folder(Some(projects.id), "  Kitchen\n")
            .unwrap();
        assert_eq!(garden.position, 0);
        assert_eq!(kitchen.position, 1);
        assert_eq!(kitchen.name, "Kitchen");
        assert_eq!(store.list_folders().unwrap().len(), 3);
    }

    #[test]
    fn rejects_empty_names() {
        let (mut store, _) = store();
        assert!(store.create_folder(None, "   ").is_err());
    }

    #[test]
    fn refuses_to_move_a_folder_into_its_own_subtree() {
        let (mut store, _) = store();
        let a = store.create_folder(None, "a").unwrap();
        let b = store.create_folder(Some(a.id), "b").unwrap();
        let c = store.create_folder(Some(b.id), "c").unwrap();
        assert!(store.move_folder(a.id, Some(c.id)).is_err());
        assert!(store.move_folder(a.id, Some(a.id)).is_err());
        store.move_folder(c.id, None).unwrap();
        assert_eq!(store.folder(c.id).unwrap().parent_id, None);
    }

    #[test]
    fn reorders_siblings() {
        let (mut store, _) = store();
        let a = store.create_folder(None, "a").unwrap();
        let b = store.create_folder(None, "b").unwrap();
        store.reorder_folders(None, &[b.id, a.id]).unwrap();
        assert_eq!(store.folder(b.id).unwrap().position, 0);
        assert_eq!(store.folder(a.id).unwrap().position, 1);
        let child = store.create_folder(Some(a.id), "c").unwrap();
        assert!(store.reorder_folders(None, &[child.id]).is_err());
    }
}
