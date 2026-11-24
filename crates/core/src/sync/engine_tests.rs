// SPDX-License-Identifier: AGPL-3.0-or-later

//! Sync rounds against a server in memory that keeps changes like the real
//! one (numbered, a change id kept once) and can fail the ways a network
//! does: no answer, an answer lost after the work was done, pages sent
//! twice. Standard and e2ee accounts run the same tests.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Mutex;

use app_e2ee::MasterKey;
use app_protocol::e2ee::AccountKind;
use app_protocol::sync::{IncomingChange, PullAnswer, PushAnswer, PushRequest};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use std::io::Read as _;

use super::engine::{Access, Backoff, Remote, RemoteError, run};
use super::files::{FileError, FilePart};
use crate::id::NoteId;
use crate::store::{NewLink, NewNote, NoteChange, Store, Tokens};
use crate::testing::{Rng, random_change, view};

/// One account's log of changes.
#[derive(Default)]
struct Log {
    changes: Vec<IncomingChange>,
    ids: HashSet<String>,
    /// Numbers of changes emptied by a compaction.
    compacted: HashSet<i64>,
    /// Files of the space by address, as the server keeps them.
    files: HashMap<String, MemFile>,
}

/// A file on the server in memory: the bytes received, its whole size,
/// whether the space let it go.
#[derive(Default, Clone)]
struct MemFile {
    data: Vec<u8>,
    total: u64,
    complete: bool,
    released: bool,
}

impl Log {
    /// Changes not compacted, by document.
    fn live(&self, doc: &str) -> usize {
        self.changes
            .iter()
            .filter(|c| c.doc == doc && !self.compacted.contains(&c.seq))
            .count()
    }
}

/// What the next calls do wrong.
#[derive(Default, Clone, Copy)]
struct Faults {
    /// Fail without doing anything.
    offline: bool,
    /// Do the work, then lose the answer.
    lose_answer: bool,
    /// Downloads stop halfway.
    cut: bool,
}

struct Server {
    log: Rc<RefCell<Log>>,
    device: String,
    faults: Faults,
}

impl Remote for Server {
    fn push(&mut self, request: &PushRequest) -> Result<PushAnswer, RemoteError> {
        if self.faults.offline {
            return Err(RemoteError::Offline("no network".into()));
        }
        let mut log = self.log.borrow_mut();
        let mut accepted = 0;
        for change in &request.changes {
            if log.ids.insert(change.id.clone()) {
                let seq = i64::try_from(log.changes.len()).unwrap() + 1;
                log.changes.push(IncomingChange {
                    seq,
                    id: change.id.clone(),
                    doc: change.doc.clone(),
                    data: change.data.clone(),
                    device: Some(self.device.clone()),
                });
                accepted += 1;
                if let Some(up_to) = change.compacts {
                    let emptied: Vec<i64> = log
                        .changes
                        .iter()
                        .filter(|c| c.doc == change.doc && c.seq <= up_to)
                        .map(|c| c.seq)
                        .collect();
                    log.compacted.extend(emptied);
                }
            }
        }
        if self.faults.lose_answer {
            return Err(RemoteError::Offline("answer lost".into()));
        }
        Ok(PushAnswer {
            accepted,
            duplicates: u32::try_from(request.changes.len()).unwrap() - accepted,
            head: i64::try_from(log.changes.len()).unwrap(),
        })
    }

    fn pull(&mut self, after: i64, limit: usize) -> Result<PullAnswer, RemoteError> {
        if self.faults.offline {
            return Err(RemoteError::Offline("no network".into()));
        }
        let log = self.log.borrow();
        let start = usize::try_from(after).unwrap().min(log.changes.len());
        let mut end = start;
        let mut page = Vec::new();
        while end < log.changes.len() && page.len() < limit {
            if !log.compacted.contains(&log.changes[end].seq) {
                page.push(log.changes[end].clone());
            }
            end += 1;
        }
        if self.faults.lose_answer {
            return Err(RemoteError::Offline("answer lost".into()));
        }
        let anchor = if after > 0 {
            log.changes
                .iter()
                .find(|c| c.seq == after)
                .map(|c| c.id.clone())
        } else {
            None
        };
        Ok(PullAnswer {
            changes: page,
            keys: vec![],
            head: i64::try_from(log.changes.len()).unwrap(),
            more: end < log.changes.len(),
            anchor,
        })
    }

    fn heavy(&mut self, min: i64) -> Result<app_protocol::sync::HeavyDocs, RemoteError> {
        if self.faults.offline {
            return Err(RemoteError::Offline("no network".into()));
        }
        let log = self.log.borrow();
        let mut docs: Vec<String> = log.changes.iter().map(|c| c.doc.clone()).collect();
        docs.sort();
        docs.dedup();
        Ok(app_protocol::sync::HeavyDocs {
            docs: docs
                .into_iter()
                .map(|doc| app_protocol::sync::HeavyDoc {
                    changes: i64::try_from(log.live(&doc)).unwrap(),
                    doc,
                })
                .filter(|d| d.changes >= min)
                .collect(),
        })
    }

    fn files_info(&mut self) -> Result<Option<app_protocol::files::FilesInfo>, RemoteError> {
        if self.faults.offline {
            return Err(RemoteError::Offline("no network".into()));
        }
        Ok(Some(app_protocol::files::FilesInfo {
            policy: app_protocol::files::FilesPolicy {
                max_file_bytes: 0,
                quota_bytes: 0,
                allowed_types: vec![],
                blocked_types: vec![],
                max_files_per_note: 0,
                image_max_px: 0,
                image_quality: 85,
                thumbnails: true,
                retention_days: 30,
            },
            usage: app_protocol::files::FilesUsage {
                used_bytes: 0,
                file_bytes: 0,
                files: 0,
            },
            // small parts, so uploads come in several and get cut between them
            part_bytes: 2 * app_e2ee::CHUNK as u64,
        }))
    }

    fn file_state(&mut self, address: &str) -> Result<app_protocol::files::FileState, FileError> {
        use app_protocol::files::{FileState, FileStatus};
        if self.faults.offline {
            return Err(RemoteError::Offline("no network".into()).into());
        }
        let log = self.log.borrow();
        Ok(match log.files.get(address) {
            None => FileState {
                status: FileStatus::None,
                received: 0,
                size: 0,
            },
            Some(f) => FileState {
                status: match (f.complete, f.released) {
                    (true, false) => FileStatus::Complete,
                    (true, true) => FileStatus::Kept,
                    _ => FileStatus::Partial,
                },
                received: f.data.len() as u64,
                size: f.total,
            },
        })
    }

    fn put_file(
        &mut self,
        address: &str,
        part: FilePart<'_>,
    ) -> Result<app_protocol::files::FileState, FileError> {
        if self.faults.offline {
            return Err(RemoteError::Offline("no network".into()).into());
        }
        {
            let mut log = self.log.borrow_mut();
            let file = log.files.entry(address.to_owned()).or_default();
            if !file.complete || file.released {
                if part.start == 0 {
                    *file = MemFile {
                        total: part.total,
                        ..MemFile::default()
                    };
                }
                if part.start != file.data.len() as u64 || part.total != file.total {
                    return Err(FileError::Refused(
                        app_protocol::error_code::UPLOAD_MOVED.into(),
                    ));
                }
                file.data.extend_from_slice(part.bytes);
                if file.data.len() as u64 == file.total {
                    file.complete = true;
                    file.released = false;
                }
            }
        }
        if self.faults.lose_answer {
            return Err(RemoteError::Offline("answer lost".into()).into());
        }
        self.file_state(address)
    }

    fn get_file(&mut self, address: &str) -> Result<Box<dyn std::io::Read + Send>, FileError> {
        if self.faults.offline {
            return Err(RemoteError::Offline("no network".into()).into());
        }
        let log = self.log.borrow();
        let file = log
            .files
            .get(address)
            .filter(|f| f.complete)
            .ok_or(FileError::Missing)?;
        let data = file.data.clone();
        if self.faults.cut {
            // half the bytes, then the connection drops
            let half = data[..data.len() / 2].to_vec();
            let broken = std::io::Cursor::new(half).chain(Broken);
            return Ok(Box::new(broken));
        }
        Ok(Box::new(std::io::Cursor::new(data)))
    }

    fn keep_file(&mut self, address: &str) -> Result<(), FileError> {
        if self.faults.offline {
            return Err(RemoteError::Offline("no network".into()).into());
        }
        let mut log = self.log.borrow_mut();
        let file = log
            .files
            .get_mut(address)
            .filter(|f| f.complete && f.released)
            .ok_or(FileError::Missing)?;
        file.released = false;
        Ok(())
    }

    fn release_file(&mut self, address: &str, head: i64) -> Result<bool, FileError> {
        if self.faults.offline {
            return Err(RemoteError::Offline("no network".into()).into());
        }
        let mut log = self.log.borrow_mut();
        if i64::try_from(log.changes.len()).unwrap() != head {
            return Ok(false);
        }
        if let Some(file) = log.files.get_mut(address) {
            file.released = true;
        }
        if self.faults.lose_answer {
            return Err(RemoteError::Offline("answer lost".into()).into());
        }
        Ok(true)
    }
}

/// A connection that drops.
struct Broken;

impl std::io::Read for Broken {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "connection dropped",
        ))
    }
}

fn fresh() -> Store {
    Store::with_clock(
        crate::db::open_in_memory(),
        std::sync::Arc::new(crate::clock::SystemClock),
    )
}

struct Device {
    store: Mutex<Store>,
    server: Server,
}

fn tokens() -> Tokens {
    Tokens {
        access: "a".into(),
        access_expires_at: i64::MAX,
        refresh: "r".into(),
        refresh_expires_at: i64::MAX,
    }
}

/// The account: its log and, for e2ee, its key bytes.
struct Account {
    id: String,
    log: Rc<RefCell<Log>>,
    key: Option<[u8; 32]>,
}

impl Account {
    fn new(id: &str, kind: AccountKind) -> Self {
        Self {
            id: id.into(),
            log: Rc::default(),
            key: (kind == AccountKind::E2ee).then(|| *MasterKey::generate().as_bytes()),
        }
    }

    fn link(&self, store: &mut Store, device: &str) {
        store
            .sync_set_link(&NewLink {
                server: "https://notes.test".into(),
                account_id: self.id.clone(),
                username: "sam".into(),
                kind: if self.key.is_some() {
                    AccountKind::E2ee
                } else {
                    AccountKind::Standard
                },
                master_key: self.key.map(|k| MasterKey::from_slice(&k).unwrap()),
                device_id: device.into(),
                tokens: tokens(),
            })
            .unwrap();
    }

    fn device(&self, name: &str) -> Device {
        let mut store = fresh();
        self.link(&mut store, name);
        self.device_with(store, name)
    }

    fn device_with(&self, store: Store, name: &str) -> Device {
        Device {
            store: Mutex::new(store),
            server: Server {
                log: self.log.clone(),
                device: name.into(),
                faults: Faults::default(),
            },
        }
    }
}

impl Device {
    fn sync(&mut self) -> super::engine::Round {
        run(&self.store, &mut self.server).unwrap()
    }

    fn s(&self) -> std::sync::MutexGuard<'_, Store> {
        self.store.lock().unwrap()
    }

    fn note(&self, title: &str, body: &str) -> NoteId {
        self.s()
            .create_note(&NewNote {
                title: title.into(),
                body: body.into(),
                ..NewNote::default()
            })
            .unwrap()
            .id
    }

    fn view(&self) -> String {
        view(&self.s())
    }
}

fn same(a: &Device, b: &Device) {
    let (va, vb) = (a.view(), b.view());
    assert_eq!(va, vb, "devices differ\n--- a\n{va}\n--- b\n{vb}");
}

fn both_kinds(test: impl Fn(AccountKind)) {
    test(AccountKind::Standard);
    test(AccountKind::E2ee);
}

#[test]
fn two_devices_meet_through_the_server() {
    both_kinds(|kind| {
        let account = Account::new("acct", kind);
        let mut laptop = account.device("laptop");
        let mut phone = account.device("phone");
        let garden = laptop.s().create_folder(None, "Garden").unwrap();
        let id = laptop.note("Roses", "prune in march");
        laptop.s().move_note(id, Some(garden.id)).unwrap();
        laptop.s().set_pinned(id, true).unwrap();

        let round = laptop.sync();
        assert!(round.pushed > 0);
        assert_eq!(round.taken.own, round.pushed, "its own changes are skipped");
        assert_eq!(round.taken.applied, 0);
        let round = phone.sync();
        assert!(round.taken.applied > 0);
        same(&laptop, &phone);

        // nothing new: a round sends and takes nothing
        let round = phone.sync();
        assert_eq!((round.pushed, round.taken.applied), (0, 0));
        assert_eq!(phone.s().sync_counts().unwrap().pending, 0);
        let link = phone.s().sync_link().unwrap().unwrap();
        assert_eq!(
            link.cursor,
            i64::try_from(account.log.borrow().changes.len()).unwrap()
        );
    });
}

#[test]
fn e2ee_changes_reach_the_server_unreadable() {
    let account = Account::new("acct", AccountKind::E2ee);
    let mut laptop = account.device("laptop");
    laptop.note("Secret plan", "the treasure is under the oak");
    laptop.s().create_folder(None, "Private folder").unwrap();
    laptop.sync();
    for change in &account.log.borrow().changes {
        let bytes = STANDARD.decode(&change.data).unwrap();
        for word in ["treasure", "Secret plan", "Private folder"] {
            assert!(
                !bytes.windows(word.len()).any(|w| w == word.as_bytes()),
                "{word} readable on the server"
            );
        }
    }

    // a standard account's changes are plain Loro updates for the server
    // to encrypt at rest
    let standard = Account::new("other", AccountKind::Standard);
    let mut desk = standard.device("desk");
    desk.note("Open plan", "the treasure is under the oak");
    desk.sync();
    let readable = standard.log.borrow().changes.iter().any(|c| {
        let bytes = STANDARD.decode(&c.data).unwrap();
        bytes.windows(8).any(|w| w == b"treasure")
    });
    assert!(readable);
}

#[test]
fn a_device_with_the_wrong_key_sets_changes_aside_and_goes_on() {
    let account = Account::new("acct", AccountKind::E2ee);
    let mut laptop = account.device("laptop");
    laptop.note("One", "1");
    laptop.sync();

    // same log, another key: as if the key on this device were damaged
    let stranger = Account {
        id: "acct".into(),
        log: account.log.clone(),
        key: Some(*MasterKey::generate().as_bytes()),
    };
    let mut odd = stranger.device("odd");
    let round = odd.sync();
    assert!(round.taken.rejected > 0);
    assert_eq!(round.taken.applied, 0);
    assert_eq!(
        odd.s().sync_rejected().unwrap(),
        i64::from(round.taken.rejected)
    );
    assert!(odd.view().is_empty() || !odd.view().contains("One"));
    // the cursor moved on: the round does not take them again
    assert_eq!(odd.sync().taken.rejected, 0);
}

#[test]
fn lost_answers_and_outages_never_lose_or_double_anything() {
    both_kinds(|kind| {
        let account = Account::new("acct", kind);
        let mut a = account.device("a");
        let mut b = account.device("b");
        let id = a.note("List", "eggs");
        a.sync();
        b.sync();

        // the push lands, the answer is lost: the outbox stays and goes again
        a.s()
            .update_note(
                id,
                NoteChange {
                    body: Some("eggs\nflour".into()),
                    ..NoteChange::default()
                },
            )
            .unwrap();
        a.server.faults.lose_answer = true;
        assert!(run(&a.store, &mut a.server).is_err());
        assert!(a.s().sync_counts().unwrap().pending > 0);
        let stored = account.log.borrow().changes.len();
        a.server.faults.lose_answer = false;
        a.sync();
        assert_eq!(account.log.borrow().changes.len(), stored, "kept once");
        assert_eq!(a.s().sync_counts().unwrap().pending, 0);

        // offline: nothing moves, nothing lost
        b.server.faults.offline = true;
        b.s()
            .update_note(
                id,
                NoteChange {
                    title: Some("Shopping".into()),
                    ..NoteChange::default()
                },
            )
            .unwrap();
        assert!(matches!(
            run(&b.store, &mut b.server),
            Err(super::engine::SyncError::Remote(RemoteError::Offline(_)))
        ));
        b.server.faults.offline = false;

        // a pull whose answer is lost is simply pulled again
        b.server.faults.lose_answer = true;
        assert!(run(&b.store, &mut b.server).is_err());
        b.server.faults.lose_answer = false;
        b.sync();
        a.sync();
        same(&a, &b);
        assert!(a.view().contains("Shopping") && a.view().contains("flour"));
    });
}

#[test]
fn a_page_taken_twice_changes_nothing() {
    let account = Account::new("acct", AccountKind::Standard);
    let mut a = account.device("a");
    let b = account.device("b");
    for i in 0..5 {
        a.note(&format!("n{i}"), "x");
    }
    a.sync();
    let page = a.server.pull(0, 1000).unwrap();
    let first = b.s().sync_take_page(&page).unwrap();
    let before = b.view();
    let again = b.s().sync_take_page(&page).unwrap();
    assert!(first.applied > 0);
    assert_eq!(again, crate::store::Taken::default());
    assert_eq!(b.view(), before);
}

#[test]
fn small_pages_and_batches_cover_everything() {
    let account = Account::new("acct", AccountKind::E2ee);
    let mut a = account.device("a");
    let mut b = account.device("b");
    for i in 0..1200 {
        a.note(&format!("note {i}"), "body");
    }
    let round = a.sync();
    assert!(round.pushed >= 1200, "{round:?}");
    let round = b.sync();
    assert!(round.taken.applied >= 1200);
    same(&a, &b);
}

#[test]
fn a_vault_linked_to_another_account_sends_all_it_holds() {
    // a vault used alone for a while, then linked
    let first = Account::new("first", AccountKind::Standard);
    let mut a = first.device_with(fresh(), "a");
    let folder = a.s().create_folder(None, "Work").unwrap();
    for i in 0..30 {
        let id = a.note(&format!("n{i}"), "draft");
        for round in 0..5 {
            a.s()
                .update_note(
                    id,
                    NoteChange {
                        body: Some(format!("draft {round}")),
                        ..NoteChange::default()
                    },
                )
                .unwrap();
        }
        if i % 3 == 0 {
            a.s().move_note(id, Some(folder.id)).unwrap();
        }
    }
    let outbox = a.s().sync_counts().unwrap().pending;
    first.link(&mut a.s(), "a");
    a.sync();
    // linking was the first time: one change per document went, not every edit
    let sent = first.log.borrow().changes.len();
    assert!(
        i64::try_from(sent).unwrap() < outbox,
        "{sent} sent for {outbox} edits"
    );
    assert_eq!(sent, 31, "the space and 30 notes");

    let second = Account::new("second", AccountKind::E2ee);
    let store = a.store.into_inner().unwrap();
    let mut a = second.device_with(store, "a2");
    second.link(&mut a.s(), "a2");
    a.sync();
    let mut b = second.device("b");
    b.sync();
    same(&a, &b);
    assert!(b.view().contains("draft 4"));
}

#[test]
fn signing_in_again_to_the_same_account_picks_up_where_it_left_off() {
    let account = Account::new("acct", AccountKind::Standard);
    let mut a = account.device("a");
    let mut b = account.device("b");
    a.note("one", "1");
    a.sync();
    b.sync();
    let cursor = b.s().sync_link().unwrap().unwrap().cursor;
    b.s().sync_set_tokens(None).unwrap();
    assert!(b.s().sync_link().unwrap().unwrap().tokens.is_none());
    account.link(&mut b.s(), "b-again");
    assert_eq!(b.s().sync_link().unwrap().unwrap().cursor, cursor);
    b.note("two", "2");
    let round = b.sync();
    assert_eq!(round.pushed, 2, "only the new note and the space");
    a.sync();
    same(&a, &b);
}

#[test]
fn an_edit_saved_from_an_older_screen_keeps_the_other_devices_edit() {
    let account = Account::new("acct", AccountKind::Standard);
    let mut a = account.device("a");
    let mut b = account.device("b");
    let id = a.note("Plan", "one\ntwo\nthree");
    a.sync();
    b.sync();
    // b's editor opened the note now
    let base = b.s().note(id).unwrap().body;
    a.s()
        .update_note(
            id,
            NoteChange {
                body: Some("one\ntwo\nthree\nfour".into()),
                ..NoteChange::default()
            },
        )
        .unwrap();
    a.sync();
    b.sync();
    // the editor saves what it had plus its own line, from the old base
    let saved = b
        .s()
        .update_note(
            id,
            NoteChange {
                body: Some("zero\none\ntwo\nthree".into()),
                base_body: Some(base),
                ..NoteChange::default()
            },
        )
        .unwrap();
    assert_eq!(saved.body, "zero\none\ntwo\nthree\nfour");
    b.sync();
    a.sync();
    same(&a, &b);
}

#[test]
fn a_change_that_comes_before_the_one_it_builds_on_waits_even_across_a_restart() {
    let account = Account::new("acct", AccountKind::Standard);
    let mut a = account.device("a");
    let id = a.note("Draft", "v1");
    a.sync();
    a.s()
        .update_note(
            id,
            NoteChange {
                body: Some("v2".into()),
                ..NoteChange::default()
            },
        )
        .unwrap();
    a.sync();
    let log = account.log.borrow();
    let note_changes: Vec<&IncomingChange> = log
        .changes
        .iter()
        .filter(|c| c.doc == id.to_string())
        .collect();
    assert_eq!(note_changes.len(), 2);
    let space: Vec<&IncomingChange> = log.changes.iter().filter(|c| c.doc == "space").collect();

    let mut b = fresh();
    let bytes = |c: &IncomingChange| STANDARD.decode(&c.data).unwrap();
    for c in &space {
        b.apply_remote("space", &bytes(c)).unwrap();
    }
    b.apply_remote(&id.to_string(), &bytes(note_changes[1]))
        .unwrap();
    assert_eq!(b.waiting_for_tests(), 1);
    b.forget_docs_for_tests();
    b.apply_remote(&id.to_string(), &bytes(note_changes[0]))
        .unwrap();
    assert_eq!(b.waiting_for_tests(), 0);
    assert_eq!(b.note(id).unwrap().body, "v2");
}

#[test]
fn a_server_restored_from_an_older_backup_gets_everything_back() {
    both_kinds(|kind| {
        let account = Account::new("acct", kind);
        let mut a = account.device("a");
        let mut b = account.device("b");
        a.note("early", "1");
        a.sync();
        let backup = account.log.borrow().changes.len();
        let late = b.note("late", "2");
        b.sync();
        a.sync();
        b.s().set_pinned(late, true).unwrap();
        b.sync();
        // the server goes back to its backup: b's note and pin are gone there
        {
            let mut log = account.log.borrow_mut();
            log.changes.truncate(backup);
            let kept: HashSet<String> = HashSet::new();
            log.ids = kept;
            log.compacted
                .retain(|&seq| seq <= i64::try_from(backup).unwrap());
        }
        let round = a.sync();
        assert!(round.resent, "a noticed and sent all it holds");
        // a's resend took the log past b's cursor: b still notices, the
        // change under its number is not the one it took
        let len = i64::try_from(account.log.borrow().changes.len()).unwrap();
        assert!(len >= b.s().sync_link().unwrap().unwrap().cursor);
        let round = b.sync();
        assert!(round.resent);
        a.sync();
        same(&a, &b);
        // a third device starting now finds everything on the server
        let mut c = account.device("c");
        c.sync();
        same(&a, &c);
        assert!(c.view().contains("late"));
    });
}

#[test]
fn compaction_shrinks_the_log_and_devices_behind_still_end_the_same() {
    both_kinds(|kind| {
        let account = Account::new("acct", kind);
        let mut a = account.device("a");
        let mut b = account.device("b");
        let id = a.note("Diary", "day 0");
        a.sync();
        b.sync();
        let mut late = account.device("late");
        late.sync();
        for day in 1..=30 {
            a.s()
                .update_note(
                    id,
                    NoteChange {
                        body: Some(format!("day {day}")),
                        ..NoteChange::default()
                    },
                )
                .unwrap();
            a.sync();
        }
        let doc = id.to_string();
        let before = account.log.borrow().live(&doc);
        assert!(before > 30);
        // b catches up, then compacts what it took
        b.sync();
        let queued = super::engine::compact(&b.store, &mut b.server, 10).unwrap();
        assert!(queued >= 1, "the note, and the space it sits in");
        assert_eq!(
            super::engine::compact(&b.store, &mut b.server, 10).unwrap(),
            0,
            "queued once"
        );
        b.sync();
        let after = account.log.borrow().live(&doc);
        assert!(after <= 2, "{before} changes became {after}");
        // the late device had day 0 only: it gets the whole note at once
        late.sync();
        same(&a, &late);
        // a device starting now gets it too, and edits go on
        let mut fresh_device = account.device("new");
        fresh_device.sync();
        same(&a, &fresh_device);
        a.s()
            .update_note(
                id,
                NoteChange {
                    body: Some("day 31".into()),
                    ..NoteChange::default()
                },
            )
            .unwrap();
        a.sync();
        for d in [&mut b, &mut late, &mut fresh_device] {
            d.sync();
        }
        same(&a, &late);
        same(&a, &fresh_device);
        assert!(late.view().contains("day 31"));
    });
}

#[test]
fn a_device_that_cannot_vouch_for_a_document_does_not_compact_it() {
    let account = Account::new("acct", AccountKind::Standard);
    let mut a = account.device("a");
    let id = a.note("x", "1");
    a.sync();
    // one change of the note set aside unread here: this device has not
    // got all of it
    let stranger = Account {
        id: "acct".into(),
        log: account.log.clone(),
        key: None,
    };
    let mut b = stranger.device("b");
    b.sync();
    b.s()
        .sync_take_page(&app_protocol::sync::PullAnswer {
            changes: vec![IncomingChange {
                seq: 1000,
                id: "bad".into(),
                doc: id.to_string(),
                data: STANDARD.encode(b"not loro"),
                device: Some("other".into()),
            }],
            keys: vec![],
            head: 1000,
            more: false,
            anchor: None,
        })
        .unwrap();
    assert!(!b.s().sync_queue_compaction(&id.to_string()).unwrap());
}

#[test]
fn many_devices_with_random_edits_and_faults_end_the_same() {
    let runs: u64 = std::env::var("ENGINE_FUZZ_RUNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    let first_seed: u64 = std::env::var("ENGINE_FUZZ_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    for seed in first_seed..first_seed + runs {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let kind = if seed % 2 == 0 {
            AccountKind::E2ee
        } else {
            AccountKind::Standard
        };
        let account = Account::new("acct", kind);
        let mut devices: Vec<Device> = (0..3).map(|i| account.device(&format!("d{i}"))).collect();
        for _ in 0..60 {
            let d = rng.below(devices.len());
            match rng.below(6) {
                0 => {
                    devices[d].server.faults = Faults {
                        offline: rng.below(3) == 0,
                        lose_answer: rng.below(3) == 0,
                        cut: false,
                    };
                    let device = &mut devices[d];
                    let _ = run(&device.store, &mut device.server);
                    device.server.faults = Faults::default();
                }
                1 => {
                    let _ = devices[d].sync();
                    if rng.below(3) == 0 {
                        let device = &mut devices[d];
                        let _ = super::engine::compact(&device.store, &mut device.server, 2);
                    }
                }
                _ => random_change(&mut devices[d].s(), &mut rng),
            }
        }
        for _ in 0..2 {
            for d in &mut devices {
                d.sync();
            }
        }
        for d in &devices[1..] {
            let (va, vb) = (devices[0].view(), d.view());
            assert_eq!(va, vb, "seed {seed} ({kind:?}) differs");
        }
        for d in &devices {
            assert_eq!(d.s().sync_counts().unwrap().pending, 0, "seed {seed}");
            assert_eq!(d.s().sync_rejected().unwrap(), 0, "seed {seed}");
        }
    }
}

#[test]
fn backoff_is_used_between_failures() {
    // the loop in the apps: a failure grows the wait, a success resets it
    let mut backoff = Backoff::default();
    let account = Account::new("acct", AccountKind::Standard);
    let mut a = account.device("a");
    a.server.faults.offline = true;
    for _ in 0..3 {
        assert!(run(&a.store, &mut a.server).is_err());
        backoff.failed(1);
    }
    assert_eq!(backoff.failures(), 3);
    a.server.faults.offline = false;
    a.sync();
    backoff.succeeded();
    assert_eq!(backoff.failures(), 0);
}

#[test]
fn the_engine_runs_through_any_access() {
    // the apps keep the store behind their own lock, the engine only needs
    // short turns with it
    struct Shared(Mutex<Store>);
    impl Access for Shared {
        fn with<T>(&self, f: impl FnOnce(&mut Store) -> crate::Result<T>) -> crate::Result<T> {
            f(&mut self.0.lock().unwrap())
        }
    }
    let account = Account::new("acct", AccountKind::Standard);
    let a = account.device("a");
    a.note("x", "y");
    let shared = Shared(a.store);
    let mut server = a.server;
    let round = run(&shared, &mut server).unwrap();
    assert!(round.pushed > 0);
}

#[test]
fn notes_deleted_for_good_a_month_ago_are_forgotten_here_and_on_the_server() {
    use crate::clock::{DAY, ManualClock};
    let account = Account::new("acct", AccountKind::Standard);
    let clock = std::sync::Arc::new(ManualClock::new(1_000_000_000));
    let mut store = Store::with_clock(crate::db::open_in_memory(), clock.clone());
    account.link(&mut store, "a");
    let mut a = account.device_with(store, "a");
    let kept = a.note("Kept", "stays");
    let gone = a.note("Gone", "draft");
    for i in 0..10 {
        a.s()
            .update_note(
                gone,
                NoteChange {
                    body: Some(format!("draft {i}")),
                    ..NoteChange::default()
                },
            )
            .unwrap();
    }
    a.sync();
    let mut b = account.device("b");
    b.sync();
    a.s().trash_note(gone).unwrap();
    a.s().purge_note(gone).unwrap();
    a.sync();
    b.sync();
    let doc = gone.to_string();
    let live = |account: &Account| account.log.borrow().live(&doc);
    assert!(live(&account) > 5);

    // not yet: an edit made elsewhere could still bring it back
    assert_eq!(
        super::engine::compact(&a.store, &mut a.server, 1000).unwrap(),
        0
    );
    clock.advance(31 * DAY);
    assert_eq!(
        super::engine::compact(&a.store, &mut a.server, 1000).unwrap(),
        1
    );
    a.sync();
    assert!(
        live(&account) <= 1,
        "the server holds an empty document at most"
    );
    assert!(!a.s().doc_stored_for_tests(&doc), "forgotten here");
    // once only
    assert_eq!(
        super::engine::compact(&a.store, &mut a.server, 1000).unwrap(),
        0
    );
    // everything else as it was, everywhere, and for a new device
    b.sync();
    same(&a, &b);
    let mut c = account.device("c");
    c.sync();
    same(&a, &c);
    assert!(c.s().note(kept).is_ok());
    assert!(c.s().note(gone).is_err());
}

/// Vaults written by versions that still shared notes, with a space
/// shared to edit, one shared to read, an e2ee space this account owns
/// with a member it let in and one waiting, and an e2ee space whose key
/// never came. Every note is kept when they open, then syncs as any other.
mod old_vaults {
    use std::path::Path;

    use rusqlite::Connection;

    use super::*;
    use crate::db::{insert_any, migrate_to, schema_version};
    use crate::model::{NoteScope, NoteSort};

    const EDIT: &str = "11111111-2222-3333-4444-555555555555";
    const READ: &str = "22222222-2222-3333-4444-555555555555";
    const TRUSTED: &str = "33333333-2222-3333-4444-555555555555";
    const WAITING: &str = "44444444-2222-3333-4444-555555555555";

    const PLAN: &str = "01a16bcd-5fea-73a0-9044-6dbdccb22870";
    const SEEDS: &str = "01a16bcd-5fea-73a0-9044-6dbdccb22871";
    const TOMATOES: &str = "01a16bcd-5fea-73a0-9044-6dbdccb22872";
    const VARIETIES: &str = "01a16bcd-5fea-73a0-9044-6dbdccb22873";
    const MEETING: &str = "01a16bcd-5fea-73a0-9044-6dbdccb22874";
    const SOUPS: &str = "01a16bcd-5fea-73a0-9044-6dbdccb22875";
    const LEEK: &str = "01a16bcd-5fea-73a0-9044-6dbdccb22876";
    const BUDGET: &str = "01a16bcd-5fea-73a0-9044-6dbdccb22877";
    const FILE: &str = "01a16bcd-5fea-73a0-9044-6dbdccb22878";
    const TAG: &str = "01a16bcd-5fea-73a0-9044-6dbdccb22879";

    /// Every note of the vault, as the person reads it.
    const TITLES: [&str; 5] = [
        "Budget 2027",
        "Club meeting",
        "Leek soup",
        "Tomato varieties",
        "Weekly plan",
    ];

    fn has_table(conn: &Connection, name: &str) -> bool {
        conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [name],
            |r| r.get(0),
        )
        .unwrap()
    }

    /// A vault as the version at schema step `from` wrote it. With
    /// `documents` its sync documents are built too, as a vault that synced
    /// has them; without, the rows came in quietly and have none.
    #[expect(clippy::too_many_lines, reason = "the whole vault, read top to bottom")]
    fn old_vault(path: &Path, from: usize, documents: bool) {
        let conn = Connection::open(path).unwrap();
        migrate_to(&conn, from).unwrap();
        let note = |id: &str, title: &str, body: &str, rest: &[(&str, &str)]| {
            let mut values = vec![
                ("id", id),
                ("title", title),
                ("body", body),
                ("created_at", "1700000000000"),
                ("updated_at", "1700000000000"),
            ];
            values.extend_from_slice(rest);
            insert_any(&conn, "notes", &values);
        };
        note(PLAN, "Weekly plan", "Order more cedar posts", &[]);
        for (id, role, kind) in [
            (EDIT, "edit", "standard"),
            (READ, "read", "standard"),
            (TRUSTED, "owner", "e2ee"),
            (WAITING, "edit", "e2ee"),
        ] {
            insert_any(
                &conn,
                "spaces",
                &[
                    ("id", id),
                    ("role", role),
                    ("kind", kind),
                    ("owner_name", "alex"),
                    ("cursor", "12"),
                ],
            );
        }
        insert_any(
            &conn,
            "space_keys",
            &[
                ("space_id", TRUSTED),
                ("epoch", "1"),
                ("key", "a space key"),
            ],
        );
        if has_table(&conn, "space_trust") {
            insert_any(
                &conn,
                "space_trust",
                &[
                    ("space_id", TRUSTED),
                    ("user_id", "sam"),
                    ("public_key", "c2Ftcw"),
                ],
            );
            insert_any(
                &conn,
                "space_waiting",
                &[
                    ("space_id", TRUSTED),
                    ("user_id", "robin"),
                    ("name", "robin"),
                    ("public_key", "cm9iaW4"),
                ],
            );
        }
        insert_any(
            &conn,
            "folders",
            &[("id", SEEDS), ("name", "Seeds"), ("space_id", EDIT)],
        );
        insert_any(
            &conn,
            "folders",
            &[
                ("id", TOMATOES),
                ("parent_id", SEEDS),
                ("name", "Tomatoes"),
                ("space_id", EDIT),
            ],
        );
        note(
            VARIETIES,
            "Tomato varieties",
            "Black Krim, San Marzano",
            &[("folder_id", TOMATOES), ("space_id", EDIT)],
        );
        note(
            MEETING,
            "Club meeting",
            "Bring the seed trays",
            &[("space_id", EDIT), ("trashed_at", "5")],
        );
        insert_any(&conn, "tags", &[("id", TAG), ("name", "club")]);
        insert_any(&conn, "note_tags", &[("note_id", MEETING), ("tag_id", TAG)]);
        insert_any(
            &conn,
            "folders",
            &[("id", SOUPS), ("name", "Soups"), ("space_id", READ)],
        );
        note(
            LEEK,
            "Leek soup",
            "Two leeks, one potato",
            &[("folder_id", SOUPS), ("space_id", READ)],
        );
        note(
            BUDGET,
            "Budget 2027",
            "Seeds 40, tools 120",
            &[("space_id", TRUSTED), ("pinned", "1")],
        );
        if has_table(&conn, "attachments") {
            insert_any(
                &conn,
                "attachments",
                &[
                    ("id", FILE),
                    ("note_id", VARIETIES),
                    ("file_name", "beds.png"),
                    ("mime_type", "image/png"),
                    ("size", "4"),
                    ("sha256", &"0".repeat(64)),
                    ("epoch", "1"),
                ],
            );
        }
        // a change of the waiting space set aside: its key never came
        let waiting_doc = format!("{WAITING}/space");
        insert_any(
            &conn,
            "sync_rejected",
            &[
                ("seq", "3"),
                ("doc_id", &waiting_doc),
                ("reason", "no key for epoch 1"),
            ],
        );
        if documents {
            let now = crate::clock::Clock::now(&crate::clock::SystemClock);
            crate::sync::flush(&conn, &mut crate::sync::Docs::default(), now).unwrap();
        } else {
            conn.execute("DELETE FROM sync_touched", []).unwrap();
        }
    }

    fn opened(path: &Path) -> Store {
        Store::with_clock(
            crate::db::open(path, None).unwrap(),
            std::sync::Arc::new(crate::clock::SystemClock),
        )
    }

    fn titles(store: &Store) -> Vec<String> {
        let mut notes = store
            .list_notes(NoteScope::All, NoteSort::TitleAsc)
            .unwrap();
        notes.extend(
            store
                .list_notes(NoteScope::Trash, NoteSort::TitleAsc)
                .unwrap(),
        );
        let mut titles: Vec<String> = notes.into_iter().map(|n| n.title).collect();
        titles.sort();
        titles
    }

    fn by_title(store: &Store, title: &str) -> crate::model::Note {
        let mut notes = store
            .list_notes(NoteScope::All, NoteSort::TitleAsc)
            .unwrap();
        notes.extend(
            store
                .list_notes(NoteScope::Trash, NoteSort::TitleAsc)
                .unwrap(),
        );
        let id = notes.into_iter().find(|n| n.title == title).unwrap().id;
        store.note(id).unwrap()
    }

    /// The vaults to build: from the step that brought spaces to the last
    /// one before they went, without documents and, for the last, with.
    fn versions() -> Vec<(usize, bool)> {
        let last = schema_version() - 1;
        (8..=last)
            .map(|from| (from, false))
            .chain([(last, true)])
            .collect()
    }

    #[test]
    fn every_note_of_a_vault_with_shared_spaces_is_kept_when_it_opens() {
        for (from, documents) in versions() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("old.db");
            old_vault(&path, from, documents);
            let mut store = opened(&path);
            let at = format!("from {from}, documents {documents}");
            assert_eq!(titles(&store), TITLES, "{at}");

            let plan = by_title(&store, "Weekly plan");
            assert_eq!(
                plan.id.to_string(),
                PLAN,
                "{at}: the person's own note stays as is"
            );
            assert_eq!(plan.body, "Order more cedar posts", "{at}");
            let varieties = by_title(&store, "Tomato varieties");
            assert_eq!(varieties.body, "Black Krim, San Marzano", "{at}");
            assert_eq!(
                varieties.tags,
                ["Seeds/Tomatoes"],
                "{at}: its folders as a tag"
            );
            let folder = store.folder(varieties.folder_id.unwrap()).unwrap();
            assert_eq!(folder.name, "Tomatoes", "{at}");
            let parent = store.folder(folder.parent_id.unwrap()).unwrap();
            assert_eq!(parent.name, "Seeds", "{at}: folders as they were");
            let leek = by_title(&store, "Leek soup");
            assert_eq!(leek.tags, ["Soups"], "{at}: read only, it had no tag");
            assert!(by_title(&store, "Budget 2027").pinned, "{at}");
            let meeting = by_title(&store, "Club meeting");
            assert_eq!(meeting.tags, ["club"], "{at}");
            assert!(meeting.trashed_at.is_some(), "{at}: still in the trash");
            if from >= 10 {
                let files = store.list_attachments(varieties.id).unwrap();
                assert_eq!(files.len(), 1, "{at}: the attachment goes with its note");
                assert_eq!(files[0].file_name, "beds.png", "{at}");
            }
            for note in [&varieties, &leek, &meeting] {
                assert_eq!(note.space_id, "", "{at}: the person's own now");
            }

            // the trash counts from the opening for what was in a space's
            assert_eq!(store.purge_expired_trash().unwrap(), 0, "{at}");
            assert_eq!(titles(&store), TITLES, "{at}");
            assert_eq!(store.sync_rejected().unwrap(), 0, "{at}");
            drop(store);

            let conn = Connection::open(&path).unwrap();
            for table in ["spaces", "space_keys", "space_trust", "space_waiting"] {
                assert!(!has_table(&conn, table), "{at}: {table} is gone");
            }
            let left: i64 = conn
                .query_row(
                    "SELECT (SELECT count(*) FROM sync_docs WHERE id LIKE '%/%')
                          + (SELECT count(*) FROM sync_updates WHERE doc_id LIKE '%/%')
                          + (SELECT count(*) FROM sync_outbox WHERE doc_id LIKE '%/%')
                          + (SELECT count(*) FROM folders WHERE space_id <> '')",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(left, 0, "{at}: nothing of the spaces is left");
            drop(conn);
            // opening again changes nothing
            assert_eq!(titles(&opened(&path)), TITLES, "{at}");
        }
    }

    #[test]
    fn a_vault_with_shared_spaces_syncs_every_note_through_a_server() {
        both_kinds(|kind| {
            for (from, documents) in [(8, false), (schema_version() - 1, true)] {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("old.db");
                old_vault(&path, from, documents);
                let account = Account::new("sam", kind);
                let mut store = opened(&path);
                account.link(&mut store, "laptop");
                let mut laptop = account.device_with(store, "laptop");
                laptop.sync();
                assert!(
                    account
                        .log
                        .borrow()
                        .changes
                        .iter()
                        .all(|c| !c.doc.contains('/')),
                    "from {from}: only the account's own documents go"
                );
                let mut phone = account.device("phone");
                phone.sync();
                same(&laptop, &phone);
                assert_eq!(titles(&phone.s()), TITLES, "from {from} ({kind:?})");
                assert_eq!(
                    by_title(&phone.s(), "Tomato varieties").body,
                    "Black Krim, San Marzano"
                );
                // and back: an edit on the new device reaches the old vault
                let leek = by_title(&phone.s(), "Leek soup").id;
                phone
                    .s()
                    .update_note(
                        leek,
                        NoteChange {
                            body: Some("Two leeks, one potato, thyme".into()),
                            ..NoteChange::default()
                        },
                    )
                    .unwrap();
                phone.sync();
                laptop.sync();
                same(&laptop, &phone);
            }
        });
    }

    #[test]
    fn a_vault_with_shared_spaces_syncs_every_note_with_a_device_nearby() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        old_vault(&path, schema_version() - 1, true);
        let mut old = opened(&path);
        let mut new = fresh();
        for _ in 0..2 {
            let versions = new.peer_versions().unwrap();
            let changes = old.peer_changes(&versions).unwrap();
            new.peer_take(&changes).unwrap();
            let versions = old.peer_versions().unwrap();
            let changes = new.peer_changes(&versions).unwrap();
            old.peer_take(&changes).unwrap();
        }
        assert_eq!(view(&old), view(&new));
        assert_eq!(titles(&new), TITLES);
    }
}

mod files_deep {
    use super::*;
    use crate::blobs::BlobStore;
    use crate::model::{NoteScope, NoteSort};
    use crate::store::FileKeying;
    use crate::sync::files::transfer;

    /// A device with its files folder.
    struct Holder {
        device: Device,
        blobs: BlobStore,
        _dir: tempfile::TempDir,
    }

    thread_local! {
        /// The time every device of a run sees: the same run, the same steps.
        static CLOCK: std::sync::Arc<crate::clock::ManualClock> =
            std::sync::Arc::new(crate::clock::ManualClock::new(1_800_000_000_000));
    }

    fn holder(account: &Account, name: &str) -> Holder {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::with_clock(crate::db::open_in_memory(), CLOCK.with(Clone::clone));
        account.link(&mut store, name);
        // encrypted folders, as an encrypted vault keeps them
        let blobs = BlobStore::new(
            dir.path().join("files"),
            Some(zeroize::Zeroizing::new(
                [u8::try_from(name.len()).unwrap(); 32],
            )),
        );
        Holder {
            device: account.device_with(store, name),
            blobs,
            _dir: dir,
        }
    }

    /// A notes round then a files round, the faults on both.
    fn round(h: &mut Holder, faults: Faults) {
        h.device.server.faults = faults;
        let notes = run(&h.device.store, &mut h.device.server);
        if notes.is_ok() || faults.lose_answer {
            let _ = transfer(
                &h.device.store,
                &h.blobs,
                &mut h.device.server,
                u64::MAX,
                &mut (),
            );
        }
        h.device.server.faults = Faults::default();
    }

    fn clean(h: &mut Holder) {
        round(h, Faults::default());
        transfer(
            &h.device.store,
            &h.blobs,
            &mut h.device.server,
            u64::MAX,
            &mut (),
        )
        .unwrap();
    }

    fn notes(h: &Holder) -> Vec<NoteId> {
        h.device
            .s()
            .list_notes(NoteScope::All, NoteSort::TitleAsc)
            .unwrap()
            .into_iter()
            .map(|n| n.id)
            .collect()
    }

    /// Contents made in the test, by hash. A few sizes so files come in one
    /// or several parts, a small pool so the same file is attached twice.
    fn content(rng: &mut Rng) -> Vec<u8> {
        let seed = u8::try_from(rng.below(6)).unwrap();
        let len = [0, 900, 70_000, 200_000, 140_000, 5][usize::from(seed)];
        (0..len)
            .map(|i| u8::try_from((i * 7 + usize::from(seed) * 13) % 251).unwrap())
            .collect()
    }

    fn check_end(
        seed: u64,
        kind: AccountKind,
        sam: &Account,
        holders: &[Holder],
        made: &HashMap<String, Vec<u8>>,
    ) {
        for h in holders {
            assert_eq!(
                h.device.s().sync_counts().unwrap().pending,
                0,
                "seed {seed}"
            );
            assert_eq!(h.device.s().sync_rejected().unwrap(), 0, "seed {seed}");
        }
        let (va, vb) = (holders[0].device.view(), holders[1].device.view());
        assert_eq!(
            va, vb,
            "seed {seed} ({kind:?}): the devices differ\n--- laptop\n{va}\n--- phone\n{vb}"
        );
        for (i, h) in holders.iter().enumerate() {
            let uses = h.device.s().files_in_use().unwrap();
            let keying = h.device.s().file_keying().unwrap();
            for file in uses.iter().filter(|f| made.contains_key(&f.sha256)) {
                // every file a note here uses is here, and is what was attached
                let here = h.blobs.get(&file.sha256).unwrap();
                assert!(
                    here.as_deref() == Some(made[&file.sha256].as_slice()),
                    "seed {seed} ({kind:?}) device {i}: {} missing or wrong",
                    file.sha256,
                );
                // and the server holds it, not let go
                let address = match &keying {
                    FileKeying::Plain => file.sha256.clone(),
                    FileKeying::Sealed(key) => app_e2ee::file_address(key, &file.sha256),
                };
                let log = sam.log.borrow();
                let stored = log.files.get(&address).unwrap_or_else(|| {
                    panic!("seed {seed} ({kind:?}) device {i}: not on the server")
                });
                assert!(
                    stored.complete && !stored.released,
                    "seed {seed} device {i}: let go {}",
                    &file.sha256[..6],
                );
                if kind == AccountKind::E2ee && !made[&file.sha256].is_empty() {
                    assert_ne!(stored.data, made[&file.sha256], "seed {seed}: sealed");
                    assert_ne!(address, file.sha256);
                }
            }
        }
    }

    #[test]
    #[expect(clippy::too_many_lines, reason = "one random run, read top to bottom")]
    fn devices_attaching_and_removing_files_with_faults_end_with_every_file() {
        let runs: u64 = std::env::var("FILES_FUZZ_RUNS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(6);
        let first_seed: u64 = std::env::var("FILES_FUZZ_SEED")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);
        for seed in first_seed..first_seed + runs {
            let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            let kind = if seed % 2 == 0 {
                AccountKind::E2ee
            } else {
                AccountKind::Standard
            };
            let sam = Account::new("sam", kind);
            let mut holders = vec![holder(&sam, "laptop"), holder(&sam, "phone")];
            for h in &mut holders {
                clean(h);
            }
            let mut made: HashMap<String, Vec<u8>> = HashMap::new();
            for _ in 0..70 {
                CLOCK.with(|c| c.advance(i64::try_from(rng.below(5)).unwrap()));
                let d = rng.below(holders.len());
                let op = rng.below(11);
                if op == 7 {
                    // removed files forgotten, as after the retention time:
                    // by then every device has long synced
                    for _ in 0..2 {
                        for other in &mut holders {
                            clean(other);
                        }
                    }
                    let h = &mut holders[d];
                    let mut store = h.device.s();
                    store.forget_removed_files(0).unwrap();
                    let used = store.referenced_blobs().unwrap();
                    drop(store);
                    h.blobs.remove_unreferenced(&used).unwrap();
                    continue;
                }
                let h = &mut holders[d];
                match op {
                    0 | 1 => {
                        let faults = Faults {
                            offline: rng.below(4) == 0,
                            lose_answer: rng.below(3) == 0,
                            cut: rng.below(3) == 0,
                        };
                        round(h, faults);
                    }
                    2 => round(h, Faults::default()),
                    3..=5 => {
                        let list = notes(h);
                        let Some(&note) = list.get(rng.below(list.len().max(1))) else {
                            h.device.note("Holder", "");
                            continue;
                        };
                        let bytes = content(&mut rng);
                        let sha = h.blobs.put(&bytes).unwrap();
                        made.insert(sha.clone(), bytes.clone());
                        let added = h.device.s().add_attachment(
                            note,
                            "file.bin",
                            "application/octet-stream",
                            i64::try_from(bytes.len()).unwrap(),
                            &sha,
                        );
                        if let (Ok(a), true) = (added, rng.below(3) == 0) {
                            let thumb = content(&mut rng);
                            let thumb_sha = h.blobs.put(&thumb).unwrap();
                            made.insert(thumb_sha.clone(), thumb);
                            let _ = h.device.s().set_attachment_thumb(a.id, &thumb_sha);
                        }
                    }
                    6 => {
                        let files: Vec<_> = notes(h)
                            .into_iter()
                            .flat_map(|n| h.device.s().list_attachments(n).unwrap())
                            .collect();
                        if let Some(a) = files.get(rng.below(files.len().max(1))) {
                            let _ = h.device.s().remove_attachment(a.id);
                        }
                    }
                    _ => random_change(&mut h.device.s(), &mut rng),
                }
                for (i, other) in holders.iter().enumerate() {
                    let unsaved = other.device.s().unsaved_docs_for_tests();
                    assert!(
                        unsaved.is_empty(),
                        "seed {seed}: op {op} on d{d} left d{i} with unsaved {unsaved:?}"
                    );
                }
            }
            // until a whole pass sends nothing: what a round writes (a name
            // settled) goes with the next one
            let mut passes = 0;
            loop {
                let mut sent = 0;
                for h in &mut holders {
                    sent += h.device.s().sync_counts().unwrap().pending;
                    clean(h);
                    sent += h.device.s().sync_counts().unwrap().pending;
                }
                passes += 1;
                if sent == 0 && passes >= 2 {
                    break;
                }
                assert!(
                    passes < 8,
                    "seed {seed} ({kind:?}): devices keep writing, they never settle"
                );
            }
            check_end(seed, kind, &sam, &holders, &made);
        }
    }
}
