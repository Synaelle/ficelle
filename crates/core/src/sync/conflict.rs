// SPDX-License-Identifier: AGPL-3.0-or-later

//! Suspicious merges. Two devices editing the same note apart always merge,
//! nothing is lost, and most merges read fine: different paragraphs, a word
//! here and a line there. A merge is suspicious when both devices rewrote
//! most of the same passage, or one rewrote what the other deleted, or both
//! retitled the note: the merged text then holds both rewrites spliced
//! together. The merge is kept, both versions from before it go to the
//! note's history, and the note asks the person to keep one, the other,
//! both, or the merge.

use rusqlite::{Connection, params};

use super::diff::matches;
use crate::clock::Timestamp;
use crate::error::Result;
use crate::id::NoteId;
use crate::model::VersionReason;

/// A note's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Texts {
    pub title: String,
    pub body: String,
}

/// A passage of the base one side replaced or deleted: base lines
/// `start..end`, and what took their place.
struct Rewrite {
    start: usize,
    end: usize,
    old: String,
    new: String,
}

/// The base lines between two kept ones that `side` changed. Lines only
/// added keep the base, they merge side by side.
fn rewrites(base: &str, side: &str) -> Vec<Rewrite> {
    let old: Vec<&str> = base.split_inclusive('\n').collect();
    let new: Vec<&str> = side.split_inclusive('\n').collect();
    let mut kept = matches(&old, &new);
    // the end counts as kept, so a change at the end is found too
    kept.push((old.len(), new.len()));
    let mut from = (0, 0);
    let mut found = Vec::new();
    for (i, j) in kept {
        if i > from.0 {
            found.push(Rewrite {
                start: from.0,
                end: i,
                old: old[from.0..i].concat(),
                new: new[from.1..j].concat(),
            });
        }
        from = (i + 1, j + 1);
    }
    found
}

/// How much of `old` is gone in `new`, 0 to 1, by characters: the share of
/// both that a shortest edit does not keep.
fn changed(old: &str, new: &str) -> f32 {
    if old.trim().is_empty() && new.trim().is_empty() {
        return 0.0;
    }
    let old: Vec<char> = old.chars().collect();
    let new: Vec<char> = new.chars().collect();
    let total = old.len() + new.len();
    #[expect(
        clippy::cast_precision_loss,
        reason = "a ratio, notes are far below 2^24 characters"
    )]
    let kept = (2 * matches(&old, &new).len()) as f32 / total as f32;
    1.0 - kept
}

/// A passage counts as rewritten past this much change: typing a few words
/// into the same line on both devices is co-editing, not a conflict.
const REWRITTEN: f32 = 0.5;

/// Whether merging `mine` and `theirs`, both made from `base`, deserves a
/// look.
pub(crate) fn suspicious(base: &Texts, mine: &Texts, theirs: &Texts) -> bool {
    let retitled = |side: &Texts| side.title != base.title;
    if retitled(mine) && retitled(theirs) && mine.title != theirs.title {
        return true;
    }
    let ours = rewrites(&base.body, &mine.body);
    let other = rewrites(&base.body, &theirs.body);
    ours.iter().any(|a| {
        other.iter().any(|b| {
            let overlap = a.start < b.end && b.start < a.end;
            overlap
                && changed(&a.old, &a.new) >= REWRITTEN
                && changed(&b.old, &b.new) >= REWRITTEN
                && a.new != b.new
        })
    })
}

/// What a conflict is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    /// Both devices rewrote the same passage, see [`suspicious`].
    Merge,
    /// Deleted on one device while edited on another: back in the trash
    /// with the edit.
    EditedAfterDelete,
}

impl ConflictKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::EditedAfterDelete => "editedAfterDelete",
        }
    }

    pub(crate) fn parse(text: &str) -> Self {
        if text == "editedAfterDelete" {
            Self::EditedAfterDelete
        } else {
            Self::Merge
        }
    }
}

/// Keeps a suspicious merge's two sides: in the history, and as the
/// conflict the note shows until someone decides.
pub(crate) fn record_merge(
    conn: &Connection,
    note: NoteId,
    mine: &Texts,
    theirs: &Texts,
    now: Timestamp,
) -> Result<()> {
    if !exists(conn, note)? {
        // not shown here yet (its place in the space has not come) or
        // deleted for good: nobody here to ask
        return Ok(());
    }
    crate::store::history_save(
        conn,
        note,
        &mine.title,
        &mine.body,
        VersionReason::MineBeforeMerge,
        now,
    )?;
    crate::store::history_save(
        conn,
        note,
        &theirs.title,
        &theirs.body,
        VersionReason::TheirsBeforeMerge,
        now,
    )?;
    conn.execute(
        "INSERT OR REPLACE INTO note_conflicts
             (note_id, kind, mine_title, mine_body, theirs_title, theirs_body, at)
         VALUES (?1, 'merge', ?2, ?3, ?4, ?5, ?6)",
        params![note, mine.title, mine.body, theirs.title, theirs.body, now],
    )?;
    Ok(())
}

/// Marks a note edited elsewhere while it was deleted here. A merge
/// conflict already there stays: it holds more.
pub(crate) fn record_edited_after_delete(
    conn: &Connection,
    note: NoteId,
    now: Timestamp,
) -> Result<()> {
    if !exists(conn, note)? {
        return Ok(());
    }
    conn.execute(
        "INSERT OR IGNORE INTO note_conflicts (note_id, kind, at)
         VALUES (?1, 'editedAfterDelete', ?2)",
        params![note, now],
    )?;
    Ok(())
}

fn exists(conn: &Connection, note: NoteId) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM notes WHERE id = ?1)",
        [note],
        |row| row.get(0),
    )?)
}

/// A change made after the merge reached this device: someone already
/// acted on it, the question is stale.
pub(crate) fn settled(conn: &Connection, note: NoteId) -> Result<()> {
    conn.execute(
        "DELETE FROM note_conflicts WHERE note_id = ?1 AND kind = 'merge'",
        [note],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(title: &str, body: &str) -> Texts {
        Texts {
            title: title.into(),
            body: body.into(),
        }
    }

    const BASE: &str =
        "# Trip\n\nWe leave on Monday morning by train from the north station.\n\nPack light.\n";

    #[test]
    fn different_passages_and_small_edits_merge_quietly() {
        let base = texts("Trip", BASE);
        let other_paragraph = texts(
            "Trip",
            "# Trip\n\nWe leave on Monday morning by train from the north station.\n\nPack light, two bags at most.\n",
        );
        let first_paragraph = texts(
            "Trip",
            "# Trip\n\nWe leave on Tuesday morning by train from the north station.\n\nPack light.\n",
        );
        assert!(!suspicious(&base, &first_paragraph, &other_paragraph));
        // a word changed in the same line on both: co-editing
        let word = texts(
            "Trip",
            "# Trip\n\nWe leave on Monday morning by bus from the north station.\n\nPack light.\n",
        );
        assert!(!suspicious(&base, &first_paragraph, &word));
        // lines added at the same place by both are kept side by side
        let added_a = texts("Trip", &format!("{BASE}\nTickets booked.\n"));
        let added_b = texts("Trip", &format!("{BASE}\nCall the hotel.\n"));
        assert!(!suspicious(&base, &added_a, &added_b));
        // the same rewrite on both is no conflict
        assert!(!suspicious(&base, &first_paragraph, &first_paragraph));
    }

    #[test]
    fn rewrites_of_the_same_passage_deletions_and_titles_are_suspicious() {
        let base = texts("Trip", BASE);
        let mine = texts(
            "Trip",
            "# Trip\n\nPlans changed: we drive down on Sunday night with Alex.\n\nPack light.\n",
        );
        let theirs = texts(
            "Trip",
            "# Trip\n\nFlights booked, the plane leaves Friday at noon.\n\nPack light.\n",
        );
        assert!(suspicious(&base, &mine, &theirs));
        let deleted = texts("Trip", "# Trip\n\nPack light.\n");
        assert!(
            suspicious(&base, &deleted, &theirs),
            "rewritten what the other deleted"
        );
        let both_deleted = texts("Trip", "# Trip\n\nPack light.\n");
        assert!(
            !suspicious(&base, &deleted, &both_deleted),
            "deleted on both"
        );
        let retitled_a = texts("Trip to Lyon", BASE);
        let retitled_b = texts("Holiday", BASE);
        assert!(suspicious(&base, &retitled_a, &retitled_b));
        assert!(!suspicious(&base, &retitled_a, &base));
    }
}
