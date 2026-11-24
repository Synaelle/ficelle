// SPDX-License-Identifier: AGPL-3.0-or-later

//! Backups of the whole vault picked by the person, and what they hold.

use serde::Serialize;

use crate::backup::{Backup, Manifest};
use crate::error::Result;
use crate::store::Store;

/// What a backup holds and how much of it is here already.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inspected {
    manifest: Manifest,
    /// Notes of the backup already in this vault, left as they are.
    present: usize,
}

/// The name offered for a new backup made at `now`, in unix milliseconds.
pub fn default_name(now: i64) -> String {
    crate::backup::default_name(now)
        .to_string_lossy()
        .into_owned()
}

/// Reads what `backup` holds against `store`, writing nothing.
///
/// # Errors
///
/// A damaged backup, database errors.
pub fn inspect(backup: &mut Backup, store: &Store) -> Result<Inspected> {
    let data = backup.data()?;
    let ids: Vec<_> = data.notes.iter().map(|n| n.id).collect();
    let present = store.notes_present(&ids)?.len();
    Ok(Inspected {
        manifest: backup.manifest().clone(),
        present,
    })
}
