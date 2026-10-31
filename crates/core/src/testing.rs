// SPDX-License-Identifier: AGPL-3.0-or-later

//! Helpers for tests that drive several devices, here and in the server's
//! tests (feature `testing`): random changes and a full view to compare.

use crate::id::NoteId;
use crate::model::{NoteScope, NoteSort};
use crate::store::{NewNote, NoteChange, Store};

/// Deterministic xorshift, a failing seed always fails the same way.
pub struct Rng(pub u64);

impl Rng {
    /// The next number.
    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// A number below `n`.
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            usize::try_from(self.next_u64() % n as u64).unwrap_or(0)
        }
    }

    /// One of `items`.
    pub fn pick<T: Copy>(&mut self, items: &[T]) -> Option<T> {
        if items.is_empty() {
            None
        } else {
            Some(items[self.below(items.len())])
        }
    }
}

const WORDS: &[&str] = &[
    "alpha ",
    "beta\n",
    "## title\n",
    "- [ ] task\n",
    "été ",
    "[[link]] ",
    "x",
];
const TAGS: &[&str] = &["home", "Work", "work", "todo", "été"];
/// Attachment names (the same name twice, to make clashes) and the hex
/// digit their hash repeats.
const FILES: &[(&str, &str)] = &[
    ("a.png", "a"),
    ("b.png", "b"),
    ("a.png", "d"),
    ("été.pdf", "e"),
];

/// One random change on one device. Invalid ones (moving a folder into
/// itself, editing a trashed note) are refused by the store and skipped,
/// as the app would.
#[allow(
    clippy::too_many_lines,
    reason = "one arm per kind of change, it reads best as one table"
)]
///
/// # Panics
///
/// On a store error other than a refused change: it is for tests.
pub fn random_change(s: &mut Store, rng: &mut Rng) {
    let notes: Vec<NoteId> = s
        .list_notes(NoteScope::All, NoteSort::TitleAsc)
        .unwrap()
        .into_iter()
        .map(|n| n.id)
        .collect();
    let trashed: Vec<NoteId> = s
        .list_notes(NoteScope::Trash, NoteSort::TitleAsc)
        .unwrap()
        .into_iter()
        .map(|n| n.id)
        .collect();
    let folders: Vec<_> = s
        .list_folders()
        .unwrap()
        .into_iter()
        .map(|f| f.id)
        .collect();
    let all_folders: Vec<crate::id::FolderId> = s
        .list_trashed_folders_for_tests()
        .iter()
        .filter_map(|l| l.split_whitespace().nth(2).and_then(|id| id.parse().ok()))
        .chain(folders.iter().copied())
        .collect();
    let _ = match rng.below(23) {
        0 | 1 => s
            .create_note(&NewNote {
                folder_id: rng.pick(&folders).filter(|_| rng.below(2) == 0),
                title: WORDS[rng.below(WORDS.len())].trim().into(),
                body: WORDS[rng.below(WORDS.len())].into(),
                ..NewNote::default()
            })
            .map(|_| ()),
        2..=6 => match rng.pick(&notes) {
            Some(id) => {
                let body = s.note(id).unwrap().body;
                let chars: Vec<char> = body.chars().collect();
                let at = rng.below(chars.len() + 1);
                let mut new: String = chars[..at].iter().collect();
                if rng.below(3) == 0 && at < chars.len() {
                    let cut = (at + rng.below(6) + 1).min(chars.len());
                    new.extend(&chars[cut..]);
                } else {
                    new.push_str(WORDS[rng.below(WORDS.len())]);
                    new.extend(&chars[at..]);
                }
                s.update_note(
                    id,
                    NoteChange {
                        body: Some(new),
                        ..NoteChange::default()
                    },
                )
                .map(|_| ())
            }
            None => Ok(()),
        },
        7 => match rng.pick(&notes) {
            Some(id) => s
                .update_note(
                    id,
                    NoteChange {
                        title: Some(format!("t{}", rng.below(50))),
                        ..NoteChange::default()
                    },
                )
                .map(|_| ()),
            None => Ok(()),
        },
        8 => match rng.pick(&notes) {
            Some(id) => s.set_pinned(id, rng.below(2) == 0),
            None => Ok(()),
        },
        9 => match rng.pick(&notes) {
            Some(id) => s
                .move_note(id, rng.pick(&folders).filter(|_| rng.below(3) != 0))
                .map(|_| ()),
            None => Ok(()),
        },
        10 => match rng.pick(&notes) {
            Some(id) => {
                let n = rng.below(3);
                let tags: Vec<String> =
                    (0..n).map(|_| TAGS[rng.below(TAGS.len())].into()).collect();
                s.set_note_tags(id, &tags)
            }
            None => Ok(()),
        },
        11 => match rng.pick(&notes) {
            Some(id) => s.trash_note(id),
            None => Ok(()),
        },
        12 => match rng.pick(&trashed) {
            Some(id) if rng.below(2) == 0 => s.restore_note(id),
            Some(id) => s.purge_note(id),
            None => Ok(()),
        },
        13 => s
            .create_folder(rng.pick(&folders).filter(|_| rng.below(2) == 0), "folder")
            .map(|_| ()),
        14 => match rng.pick(&folders) {
            Some(f) => s.rename_folder(f, &format!("f{}", rng.below(30))),
            None => Ok(()),
        },
        15 => match rng.pick(&folders) {
            Some(f) => s
                .move_folder(f, rng.pick(&folders).filter(|_| rng.below(3) != 0))
                .map(|_| ()),
            None => Ok(()),
        },
        16 => match rng.pick(&folders) {
            Some(f) => s.trash_folder(f),
            None => Ok(()),
        },
        17 => match rng.pick(&all_folders) {
            Some(f) if rng.below(2) == 0 => s.restore_folder(f),
            Some(f) => s.purge_folder(f),
            None => Ok(()),
        },
        18 => {
            let tags: Vec<_> = s.list_tags().unwrap().into_iter().map(|t| t.id).collect();
            match rng.pick(&tags) {
                Some(t) => s.rename_tag(t, TAGS[rng.below(TAGS.len())]),
                None => Ok(()),
            }
        }
        20 => match rng.pick(&notes) {
            Some(id) => {
                let (name, hash) = FILES[rng.below(FILES.len())];
                s.add_attachment(id, name, "image/png", 5, &hash.repeat(64))
                    .map(|_| ())
            }
            None => Ok(()),
        },
        21 | 22 => {
            let files: Vec<_> = notes
                .iter()
                .flat_map(|n| s.list_attachments(*n).unwrap())
                .map(|a| a.id)
                .collect();
            match rng.pick(&files) {
                Some(a) if rng.below(3) == 0 => s.set_attachment_thumb(a, &"c".repeat(64)),
                Some(a) => s.remove_attachment(a),
                None => Ok(()),
            }
        }
        _ => {
            let mut order: Vec<_> = s
                .list_folders()
                .unwrap()
                .into_iter()
                .filter(|f| f.parent_id.is_none())
                .map(|f| f.id)
                .collect();
            if order.len() > 1 {
                let i = rng.below(order.len());
                let x = order.remove(i);
                order.insert(0, x);
            }
            s.reorder_folders(None, &order)
        }
    };
}

/// What a person sees: every note (live or trashed) and folder, by id.
///
/// # Panics
///
/// On a store error: it is for tests.
pub fn view(s: &Store) -> String {
    let mut lines = Vec::new();
    let mut notes = s.list_notes(NoteScope::All, NoteSort::TitleAsc).unwrap();
    notes.extend(s.list_notes(NoteScope::Trash, NoteSort::TitleAsc).unwrap());
    notes.sort_by_key(|n| n.id);
    for n in notes {
        let full = s.note(n.id).unwrap();
        for a in s.list_attachments(n.id).unwrap() {
            lines.push(format!(
                "file {} space={:?} note={} name={:?} sha={} size={} thumb={:?} created={}",
                a.id,
                full.space_id,
                a.note_id,
                a.file_name,
                a.sha256,
                a.size,
                a.thumb_sha256,
                a.created_at
            ));
        }
        lines.push(format!(
            "note {} space={:?} folder={:?} title={:?} body={:?} pinned={} tags={:?} trashed={:?} created={} updated={}",
            full.id,
            full.space_id,
            full.folder_id,
            full.title,
            full.body,
            full.pinned,
            full.tags,
            full.trashed_at,
            full.created_at,
            full.updated_at
        ));
    }
    let mut folders: Vec<_> = s.list_folders().unwrap();
    folders.sort_by_key(|f| f.id);
    for f in folders {
        lines.push(format!(
            "folder {} space={:?} parent={:?} name={:?} pos={} trashed={:?}",
            f.id, f.space_id, f.parent_id, f.name, f.position, f.trashed_at
        ));
    }
    let mut trashed = s.list_trashed_folders_for_tests();
    trashed.sort();
    lines.extend(trashed);
    let mut tags: Vec<String> = s
        .list_tags()
        .unwrap()
        .into_iter()
        .map(|t| format!("tag {} {}", t.name, t.note_count))
        .collect();
    tags.sort();
    lines.extend(tags);
    lines.join("\n")
}

/// The notes and folders of one space, as [`view`] shows them, for comparing
/// a shared space between the accounts in it.
pub fn space_view(s: &Store, space: &str) -> String {
    let wanted = format!("space={space:?}");
    view(s)
        .lines()
        .filter(|l| l.contains(&wanted))
        .collect::<Vec<_>>()
        .join("\n")
}
