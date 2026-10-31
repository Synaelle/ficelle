// SPDX-License-Identifier: AGPL-3.0-or-later

//! Sample content: the notes used in the design mockups, plus any number of
//! generated notes for volume and search speed tests.

use std::sync::Arc;

use app_core::clock::{Clock, DAY, MINUTE, ManualClock, SystemClock};
use app_core::id::FolderId;
use app_core::store::{NewNote, NoteChange, Store};

const TRELLIS: &str = "Six cedar posts along the south bed, jute twine strung between them. Same setup as last year but tighter spacing. See [[Garden log 2025]] for what went wrong.

## This weekend

- [x] Buy jute twine (2 rolls)
- [x] Sharpen the pruners
- [ ] Set the posts 60 cm deep
- [ ] Ask Inès for the ladder

> [!note]
> Prune suckers every week once the plants pass 30 cm.

```
spacing_cm = 45
rows = 2
posts = 6
```
";

/// Words for generated notes. French and English so accent folding gets exercised.
const WORDS: &[&str] = &[
    "garden", "twine", "ladder", "levain", "recette", "été", "hiver", "shed", "seed", "tray",
    "pruning", "tomato", "basil", "notebook", "reading", "chapter", "piranesi", "café", "marché",
    "budget", "invoice", "meeting", "trip", "train", "bicycle", "repair", "paint", "window",
];

/// Small deterministic generator (xorshift64) so generated content is the
/// same on every run, which keeps search timings comparable.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        let len = u64::try_from(items.len()).unwrap_or(1);
        items[usize::try_from(self.next() % len).unwrap_or(0)]
    }
}

struct Folders {
    inbox: FolderId,
    journal: FolderId,
    garden: FolderId,
    kitchen: FolderId,
    reading: FolderId,
}

fn folders(store: &mut Store) -> app_core::Result<Folders> {
    let projects = store.create_folder(None, "Projects")?;
    Ok(Folders {
        inbox: store.create_folder(None, "Inbox")?.id,
        journal: store.create_folder(None, "Journal")?.id,
        garden: store.create_folder(Some(projects.id), "Garden")?.id,
        kitchen: store.create_folder(Some(projects.id), "Kitchen")?.id,
        reading: store.create_folder(None, "Reading")?.id,
    })
}

fn note(folder: FolderId, title: &str, body: &str, tags: &[&str], pinned: bool) -> NewNote {
    NewNote {
        folder_id: Some(folder),
        title: title.into(),
        body: body.into(),
        tags: tags.iter().map(|t| (*t).to_owned()).collect(),
        pinned,
    }
}

/// Adds the sample notes and `generated` extra notes. Returns how many notes were added.
pub fn seed(store: &mut Store, generated: usize) -> app_core::Result<usize> {
    // dates spread over the last weeks so sorting and history look real
    let clock = Arc::new(ManualClock::new(SystemClock.now() - 21 * DAY));
    store.set_clock(clock.clone());
    let f = folders(store)?;

    let samples = [
        note(
            f.reading,
            "Reading list, autumn",
            "The Overstory, Piranesi, La Horde du Contrevent.",
            &["ideas"],
            false,
        ),
        note(
            f.inbox,
            "Ideas for the shed",
            "Pegboard wall, a shelf for seed trays, hooks for the long tools.",
            &["ideas"],
            false,
        ),
        note(
            f.journal,
            "Sync design scratch",
            "What happens when a folder moves twice offline? Both moves stay in history.",
            &["work"],
            false,
        ),
        note(
            f.inbox,
            "Call with Inès",
            "She can lend the long ladder on Saturday. Ask about the seed trays.",
            &["todo"],
            false,
        ),
        note(
            f.kitchen,
            "Pain au levain",
            "Levain at 100% hydration, 500 g flour, 350 g water, 10 g salt. Été comme hiver, 4 h de pointage.",
            &["recettes"],
            true,
        ),
        note(
            f.garden,
            "Tomato trellis plan",
            "Six cedar posts along the south bed, jute twine between them.",
            &["todo", "garden"],
            true,
        ),
    ];
    let mut added = 0;
    for sample in &samples {
        clock.advance(2 * DAY);
        let created = store.create_note(sample)?;
        added += 1;
        if created.title == "Tomato trellis plan" {
            // a second editing session, so the note has a version to restore
            clock.advance(DAY);
            store.update_note(
                created.id,
                NoteChange {
                    body: Some(TRELLIS.into()),
                    ..NoteChange::default()
                },
            )?;
        }
    }

    let targets = [f.inbox, f.journal, f.garden, f.kitchen, f.reading];
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for i in 0..generated {
        clock.advance(MINUTE);
        let title = format!("{} {} {i}", rng.pick(WORDS), rng.pick(WORDS));
        let body = (0..60)
            .map(|_| rng.pick(WORDS))
            .collect::<Vec<_>>()
            .join(" ");
        let folder = targets[i % targets.len()];
        store.create_note(&note(
            folder,
            &title,
            &body,
            &[rng.pick(&["todo", "ideas", "work"])],
            false,
        ))?;
        added += 1;
    }
    store.set_clock(Arc::new(SystemClock));
    Ok(added)
}
