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
use app_protocol::sync::{
    GiveGrants, GrantWanted, IncomingChange, PullAnswer, PushAnswer, PushRequest, SharedSpace,
    SharedSpaces, SpaceGrant, SpaceRole,
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use std::io::Read as _;

use super::engine::{Access, Backoff, Remote, RemoteError, Round, SyncError, run as engine_run};
use super::files::{FileError, FilePart};
use crate::id::NoteId;
use crate::store::{NewLink, NewNote, NoteChange, Store, Tokens};
use crate::testing::{Rng, random_change, view};

/// A sync round. The owner added its spaces' members on its own devices,
/// as the share dialog does, so they are let in first: an e2ee owner seals
/// keys for nobody else.
fn run(store: &impl Access, server: &mut Server) -> Result<Round, SyncError> {
    let owned: Vec<(String, Vec<String>)> = server
        .world
        .borrow()
        .spaces
        .iter()
        .filter(|(_, shared)| shared.owner == server.account)
        .map(|(id, shared)| {
            let members = shared.members.keys().filter(|m| **m != shared.owner);
            (id.clone(), members.cloned().collect())
        })
        .collect();
    store.with(|s| {
        for (space, members) in &owned {
            for member in members {
                if !s.space_trusted(space, member)? {
                    s.space_trust(space, member, None)?;
                }
            }
        }
        Ok(())
    })?;
    engine_run(store, server)
}

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

/// A shared space on the server in memory.
struct SharedState {
    log: Rc<RefCell<Log>>,
    kind: AccountKind,
    owner: String,
    /// Members by account id, the owner included.
    members: HashMap<String, SpaceRole>,
    /// E2ee: the latest key epoch, and the keys sealed for each member.
    epoch: u32,
    grants: HashMap<(String, u32), String>,
}

/// The server's shared spaces, seen by every account of a test.
#[derive(Default)]
struct World {
    spaces: HashMap<String, SharedState>,
    /// E2ee accounts' published keys.
    public_keys: HashMap<String, String>,
}

impl World {
    fn share(world: &Rc<RefCell<World>>, id: &str, owner: &str, kind: AccountKind) {
        world.borrow_mut().spaces.insert(
            id.into(),
            SharedState {
                log: Rc::default(),
                kind,
                owner: owner.into(),
                members: HashMap::from([(owner.to_owned(), SpaceRole::Owner)]),
                epoch: u32::from(kind == AccountKind::E2ee),
                grants: HashMap::new(),
            },
        );
    }

    fn add(world: &Rc<RefCell<World>>, space: &str, member: &str, role: SpaceRole) {
        world
            .borrow_mut()
            .spaces
            .get_mut(space)
            .unwrap()
            .members
            .insert(member.into(), role);
    }

    fn remove(world: &Rc<RefCell<World>>, space: &str, member: &str) {
        let mut world = world.borrow_mut();
        let shared = world.spaces.get_mut(space).unwrap();
        shared.members.remove(member);
        // e2ee: what is written from now on uses a key they never get
        if shared.kind == AccountKind::E2ee {
            shared.epoch += 1;
        }
    }
}

struct Server {
    log: Rc<RefCell<Log>>,
    world: Rc<RefCell<World>>,
    account: String,
    device: String,
    faults: Faults,
}

impl Server {
    /// The log a call names, if this account may use it that way.
    fn log_for(&self, space: Option<&str>, write: bool) -> Result<Rc<RefCell<Log>>, RemoteError> {
        let Some(space) = space else {
            return Ok(self.log.clone());
        };
        let world = self.world.borrow();
        let shared = world
            .spaces
            .get(space)
            .ok_or_else(|| RemoteError::Refused("404 no such space".into()))?;
        match shared.members.get(&self.account) {
            None => Err(RemoteError::Refused("403 not a member".into())),
            Some(SpaceRole::Read) if write => Err(RemoteError::Refused("403 read only".into())),
            Some(_) => Ok(shared.log.clone()),
        }
    }
}

impl Remote for Server {
    fn push(
        &mut self,
        space: Option<&str>,
        request: &PushRequest,
    ) -> Result<PushAnswer, RemoteError> {
        if self.faults.offline {
            return Err(RemoteError::Offline("no network".into()));
        }
        let log = self.log_for(space, true)?;
        let mut log = log.borrow_mut();
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

    fn pull(
        &mut self,
        space: Option<&str>,
        after: i64,
        limit: usize,
    ) -> Result<PullAnswer, RemoteError> {
        if self.faults.offline {
            return Err(RemoteError::Offline("no network".into()));
        }
        let log = self.log_for(space, false)?;
        let log = log.borrow();
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

    fn heavy(
        &mut self,
        space: Option<&str>,
        min: i64,
    ) -> Result<app_protocol::sync::HeavyDocs, RemoteError> {
        if self.faults.offline {
            return Err(RemoteError::Offline("no network".into()));
        }
        let log = self.log_for(space, false)?;
        let log = log.borrow();
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

    fn spaces(&mut self) -> Result<Option<SharedSpaces>, RemoteError> {
        if self.faults.offline {
            return Err(RemoteError::Offline("no network".into()));
        }
        let world = self.world.borrow();
        let mut spaces: Vec<SharedSpace> = world
            .spaces
            .iter()
            .filter_map(|(id, shared)| {
                shared.members.get(&self.account).map(|role| {
                    let grants = shared
                        .grants
                        .iter()
                        .filter(|((user, _), _)| *user == self.account)
                        .map(|((_, epoch), grant)| SpaceGrant {
                            epoch: *epoch,
                            grant: grant.clone(),
                        })
                        .collect();
                    let mut wanted = Vec::new();
                    if *role == SpaceRole::Owner {
                        for member in shared.members.keys() {
                            let Some(public_key) = world.public_keys.get(member) else {
                                continue;
                            };
                            for epoch in 1..=shared.epoch {
                                if !shared.grants.contains_key(&(member.clone(), epoch)) {
                                    wanted.push(GrantWanted {
                                        user_id: member.clone(),
                                        public_key: public_key.clone(),
                                        epoch,
                                        name: member.clone(),
                                    });
                                }
                            }
                        }
                    }
                    SharedSpace {
                        id: id.clone(),
                        role: *role,
                        kind: shared.kind,
                        owner: shared.owner.clone(),
                        owner_key: world
                            .public_keys
                            .get(&shared.owner)
                            .cloned()
                            .unwrap_or_default(),
                        head: i64::try_from(shared.log.borrow().changes.len()).unwrap(),
                        epoch: shared.epoch,
                        grants,
                        grants_wanted: wanted,
                    }
                })
            })
            .collect();
        spaces.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(Some(SharedSpaces { spaces }))
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

    fn file_state(
        &mut self,
        space: Option<&str>,
        address: &str,
    ) -> Result<app_protocol::files::FileState, FileError> {
        use app_protocol::files::{FileState, FileStatus};
        if self.faults.offline {
            return Err(RemoteError::Offline("no network".into()).into());
        }
        let log = self.log_for(space, false)?;
        let log = log.borrow();
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
        space: Option<&str>,
        address: &str,
        part: FilePart<'_>,
    ) -> Result<app_protocol::files::FileState, FileError> {
        if self.faults.offline {
            return Err(RemoteError::Offline("no network".into()).into());
        }
        let log = self.log_for(space, true)?;
        {
            let mut log = log.borrow_mut();
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
        self.file_state(space, address)
    }

    fn get_file(
        &mut self,
        space: Option<&str>,
        address: &str,
    ) -> Result<Box<dyn std::io::Read + Send>, FileError> {
        if self.faults.offline {
            return Err(RemoteError::Offline("no network".into()).into());
        }
        let log = self.log_for(space, false)?;
        let log = log.borrow();
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

    fn keep_file(&mut self, space: Option<&str>, address: &str) -> Result<(), FileError> {
        if self.faults.offline {
            return Err(RemoteError::Offline("no network".into()).into());
        }
        let log = self.log_for(space, true)?;
        let mut log = log.borrow_mut();
        let file = log
            .files
            .get_mut(address)
            .filter(|f| f.complete && f.released)
            .ok_or(FileError::Missing)?;
        file.released = false;
        Ok(())
    }

    fn release_file(
        &mut self,
        space: Option<&str>,
        address: &str,
        head: i64,
    ) -> Result<bool, FileError> {
        if self.faults.offline {
            return Err(RemoteError::Offline("no network".into()).into());
        }
        let log = self.log_for(space, true)?;
        let mut log = log.borrow_mut();
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

    fn give_grants(&mut self, space: &str, given: &GiveGrants) -> Result<(), RemoteError> {
        if self.faults.offline {
            return Err(RemoteError::Offline("no network".into()));
        }
        let mut world = self.world.borrow_mut();
        let shared = world
            .spaces
            .get_mut(space)
            .ok_or_else(|| RemoteError::Refused("404".into()))?;
        if shared.members.get(&self.account) != Some(&SpaceRole::Owner) {
            return Err(RemoteError::Refused("403 owners only".into()));
        }
        for grant in &given.grants {
            // the first key sealed for a member and epoch stays
            shared
                .grants
                .entry((grant.user_id.clone(), given.epoch))
                .or_insert_with(|| grant.grant.clone());
        }
        Ok(())
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

/// The account: its log, the server's shared spaces and, for e2ee, its
/// key bytes.
struct Account {
    id: String,
    log: Rc<RefCell<Log>>,
    world: Rc<RefCell<World>>,
    key: Option<[u8; 32]>,
    /// E2ee: the key pair's halves, every device of the account gets it.
    pair: Option<(String, [u8; 32])>,
}

impl Account {
    fn new(id: &str, kind: AccountKind) -> Self {
        Self::on(id, kind, &Rc::default())
    }

    /// Another account on the same server as others.
    fn on(id: &str, kind: AccountKind, world: &Rc<RefCell<World>>) -> Self {
        let pair = (kind == AccountKind::E2ee).then(|| {
            let pair = app_e2ee::new_key_pair();
            world
                .borrow_mut()
                .public_keys
                .insert(id.into(), pair.public_text());
            (pair.public_text(), *pair.private())
        });
        Self {
            id: id.into(),
            log: Rc::default(),
            world: world.clone(),
            key: (kind == AccountKind::E2ee).then(|| *MasterKey::generate().as_bytes()),
            pair,
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
        if let Some((public, private)) = &self.pair {
            store
                .sync_set_key_pair(&app_e2ee::KeyPair::from_parts(public, private).unwrap())
                .unwrap();
        }
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
                world: self.world.clone(),
                account: self.id.clone(),
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
        world: account.world.clone(),
        pair: None,
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
    let page = a.server.pull(None, 0, 1000).unwrap();
    let first = b.s().sync_take_page("", &page).unwrap();
    let before = b.view();
    let again = b.s().sync_take_page("", &page).unwrap();
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
        world: account.world.clone(),
        pair: None,
        key: None,
    };
    let mut b = stranger.device("b");
    b.sync();
    b.s()
        .sync_take_page(
            "",
            &app_protocol::sync::PullAnswer {
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
            },
        )
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

mod sharing {
    use super::*;
    use crate::model::{NoteScope, NoteSort};
    use crate::store::Item;
    use crate::testing::space_view;

    const SPACE_ID: &str = "11111111-2222-3333-4444-555555555555";

    /// Sam shares a folder with Alex (`role`): the server's space, both
    /// accounts on it, Sam's laptop with the folder and two notes in it.
    fn shared_folder(role: SpaceRole) -> (Account, Device, Device, crate::id::FolderId) {
        let world: Rc<RefCell<World>> = Rc::default();
        let sam = Account::on("sam", AccountKind::Standard, &world);
        let alex = Account::on("alex", AccountKind::Standard, &world);
        let mut laptop = sam.device("laptop");
        let folder = laptop.s().create_folder(None, "Trip").unwrap();
        for title in ["Tickets", "Packing"] {
            let id = laptop.note(title, "to do");
            laptop.s().move_note(id, Some(folder.id)).unwrap();
        }
        laptop.note("Private", "only mine");
        laptop.sync();
        World::share(&world, SPACE_ID, "sam", AccountKind::Standard);
        World::add(&world, SPACE_ID, "alex", role);
        laptop.sync();
        let moved = laptop
            .s()
            .move_to_space(Item::Folder(folder.id), SPACE_ID)
            .unwrap();
        laptop.sync();
        let phone = alex.device("phone");
        (sam, laptop, phone, moved.parse().unwrap())
    }

    fn note_in(device: &Device, folder: crate::id::FolderId, title: &str) -> NoteId {
        device
            .s()
            .list_notes(NoteScope::Folder(Some(folder)), NoteSort::TitleAsc)
            .unwrap()
            .into_iter()
            .find(|n| n.title == title)
            .unwrap()
            .id
    }

    #[test]
    fn a_shared_folder_reaches_the_member_and_their_edits_come_back() {
        let (_, mut laptop, mut phone, folder) = shared_folder(SpaceRole::Edit);
        phone.sync();
        assert!(phone.view().contains("Tickets") && phone.view().contains("Packing"));
        assert!(!phone.view().contains("Private"), "only what was shared");
        assert_eq!(
            space_view(&laptop.s(), SPACE_ID),
            space_view(&phone.s(), SPACE_ID)
        );
        // on sam's side the folder left the personal space
        assert!(!space_view(&laptop.s(), "").contains("Trip"));
        let roots = phone.s().space_roots(SPACE_ID).unwrap();
        assert_eq!((roots.folders, roots.notes.len()), (vec![folder], 0));
        let folders = laptop.s().list_folders().unwrap();
        assert!(
            folders
                .iter()
                .any(|f| f.id == folder && f.space_id == SPACE_ID)
        );

        // alex edits and adds a note in it, sam sees both
        let tickets = note_in(&phone, folder, "Tickets");
        phone
            .s()
            .update_note(
                tickets,
                NoteChange {
                    body: Some("train booked".into()),
                    ..NoteChange::default()
                },
            )
            .unwrap();
        let added = phone
            .s()
            .create_note(&NewNote {
                title: "Hotel".into(),
                body: "two nights".into(),
                folder_id: Some(folder),
                ..NewNote::default()
            })
            .unwrap();
        assert_eq!(
            added.space_id, SPACE_ID,
            "a note made in a shared folder is shared"
        );
        phone.sync();
        laptop.sync();
        assert_eq!(
            space_view(&laptop.s(), SPACE_ID),
            space_view(&phone.s(), SPACE_ID)
        );
        assert!(laptop.view().contains("train booked") && laptop.view().contains("two nights"));
    }

    #[test]
    fn the_owners_other_devices_follow_the_move() {
        let (sam, mut laptop, _, _) = shared_folder(SpaceRole::Read);
        let mut desk = sam.device("desk");
        desk.sync();
        same(&laptop, &desk);
        laptop.sync();
        same(&laptop, &desk);
        assert!(desk.view().contains(&format!("space={SPACE_ID:?}")));
    }

    #[test]
    fn a_reader_cannot_change_anything_and_the_server_would_refuse_it() {
        let (_, _, mut phone, folder) = shared_folder(SpaceRole::Read);
        phone.sync();
        let note = note_in(&phone, folder, "Tickets");
        let refused = phone.s().update_note(
            note,
            NoteChange {
                body: Some("no".into()),
                ..NoteChange::default()
            },
        );
        assert!(matches!(refused, Err(crate::Error::Invalid(_))));
        assert!(phone.s().set_pinned(note, true).is_err());
        assert!(phone.s().trash_note(note).is_err());
        assert!(
            phone
                .s()
                .create_note(&NewNote {
                    folder_id: Some(folder),
                    ..NewNote::default()
                })
                .is_err()
        );
        assert!(
            phone.s().move_to_space(Item::Note(note), "").is_err(),
            "not out of it either"
        );
        assert!(!phone.view().contains("\"no\""), "nothing changed here");
        assert_eq!(phone.s().sync_counts().unwrap().pending, 0);
        // and the server says no to a reader's push all the same
        let request = PushRequest { changes: vec![] };
        assert!(phone.server.push(Some(SPACE_ID), &request).is_err());
    }

    #[test]
    fn a_removed_member_loses_the_space_and_all_of_it() {
        let (_, mut laptop, mut phone, _) = shared_folder(SpaceRole::Edit);
        phone.sync();
        assert!(phone.view().contains("Tickets"));
        World::remove(&laptop.server.world, SPACE_ID, "alex");
        phone.sync();
        assert!(!phone.view().contains("Tickets"), "gone from the vault");
        assert!(phone.s().shared_spaces().unwrap().is_empty());
        assert!(!phone.s().doc_stored_for_tests(&format!("{SPACE_ID}/space")));
        laptop.sync();
        assert!(laptop.view().contains("Tickets"), "the owner keeps it");
    }

    #[test]
    fn stopping_sharing_brings_everything_back_to_the_owner() {
        let (_, mut laptop, mut phone, folder) = shared_folder(SpaceRole::Edit);
        phone.sync();
        let back = laptop.s().move_to_space(Item::Folder(folder), "").unwrap();
        laptop.sync();
        laptop.server.world.borrow_mut().spaces.remove(SPACE_ID);
        laptop.sync();
        phone.sync();
        assert!(!phone.view().contains("Tickets"));
        let mine = space_view(&laptop.s(), "");
        assert!(mine.contains("Tickets") && mine.contains("Packing") && mine.contains("Trip"));
        assert!(
            laptop
                .s()
                .list_folders()
                .unwrap()
                .iter()
                .any(|f| f.id.to_string() == back)
        );
        assert!(laptop.s().shared_spaces().unwrap().is_empty());
    }

    #[test]
    fn an_edit_made_before_the_move_was_seen_is_kept() {
        let world: Rc<RefCell<World>> = Rc::default();
        let sam = Account::on("sam", AccountKind::Standard, &world);
        let mut laptop = sam.device("laptop");
        let mut desk = sam.device("desk");
        let id = laptop.note("Plan", "first");
        laptop.sync();
        desk.sync();
        World::share(&world, SPACE_ID, "sam", AccountKind::Standard);
        laptop.sync();
        desk.sync();
        // the desk edits while the laptop moves the note into the share
        desk.s()
            .update_note(
                id,
                NoteChange {
                    body: Some("first, then more".into()),
                    ..NoteChange::default()
                },
            )
            .unwrap();
        laptop.s().move_to_space(Item::Note(id), SPACE_ID).unwrap();
        laptop.sync();
        desk.sync();
        laptop.sync();
        same(&laptop, &desk);
        // the shared copy and the original with the edit, nothing lost
        let all = laptop.view();
        assert!(all.contains("first, then more"), "{all}");
        assert!(space_view(&laptop.s(), SPACE_ID).contains("\"first\""));
        assert_eq!(
            laptop.s().note_conflict(id).unwrap().map(|c| c.kind),
            Some("editedAfterDelete")
        );
    }

    #[test]
    fn history_tags_and_versions_go_with_a_moved_note() {
        let world: Rc<RefCell<World>> = Rc::default();
        let sam = Account::on("sam", AccountKind::Standard, &world);
        let mut laptop = sam.device("laptop");
        World::share(&world, SPACE_ID, "sam", AccountKind::Standard);
        laptop.sync();
        let id = laptop.note("Recipe", "flour");
        laptop.s().set_note_tags(id, &["Baking".into()]).unwrap();
        laptop.s().set_pinned(id, true).unwrap();
        laptop
            .s()
            .update_note(
                id,
                NoteChange {
                    body: Some("flour, eggs".into()),
                    ..NoteChange::default()
                },
            )
            .unwrap();
        let versions = laptop.s().list_versions(id).unwrap().len();
        let moved: NoteId = laptop
            .s()
            .move_to_space(Item::Note(id), SPACE_ID)
            .unwrap()
            .parse()
            .unwrap();
        let note = laptop.s().note(moved).unwrap();
        assert_eq!(note.body, "flour, eggs");
        assert!(note.pinned);
        assert_eq!(note.tags, vec!["Baking".to_owned()]);
        assert_eq!(laptop.s().list_versions(moved).unwrap().len(), versions);
        assert!(laptop.s().note(id).is_err(), "the original is gone");
    }
}

mod e2ee_sharing {
    use super::*;
    use crate::model::{NoteScope, NoteSort};
    use crate::store::Item;
    use crate::testing::space_view;

    const SPACE_ID: &str = "aaaaaaaa-2222-3333-4444-555555555555";

    fn readable(log: &Log, words: &[&str]) -> bool {
        log.changes.iter().any(|c| {
            let bytes = STANDARD.decode(&c.data).unwrap();
            words
                .iter()
                .any(|w| bytes.windows(w.len()).any(|x| x == w.as_bytes()))
        })
    }

    #[test]
    fn an_e2ee_share_is_read_by_its_members_only_and_never_by_the_server() {
        let world: Rc<RefCell<World>> = Rc::default();
        let sam = Account::on("sam", AccountKind::E2ee, &world);
        let alex = Account::on("alex", AccountKind::E2ee, &world);
        let mut laptop = sam.device("laptop");
        let folder = laptop.s().create_folder(None, "Secret trip").unwrap();
        let id = laptop.note("Passports", "numbers in the safe");
        laptop.s().move_note(id, Some(folder.id)).unwrap();
        World::share(&world, SPACE_ID, "sam", AccountKind::E2ee);
        World::add(&world, SPACE_ID, "alex", SpaceRole::Edit);
        laptop.sync();
        assert_eq!(laptop.s().space_key_epochs(SPACE_ID).unwrap(), vec![1]);
        laptop
            .s()
            .move_to_space(Item::Folder(folder.id), SPACE_ID)
            .unwrap();
        laptop.sync();

        let mut phone = alex.device("phone");
        phone.sync();
        assert_eq!(
            space_view(&laptop.s(), SPACE_ID),
            space_view(&phone.s(), SPACE_ID)
        );
        assert!(phone.view().contains("numbers in the safe"));
        let log = world.borrow().spaces[SPACE_ID].log.clone();
        assert!(
            !readable(
                &log.borrow(),
                &["numbers in the safe", "Secret trip", "Passports"]
            ),
            "the server reads nothing of it"
        );

        // sam's other device opens the key from its own grant
        let mut desk = sam.device("desk");
        desk.sync();
        same(&laptop, &desk);
    }

    #[test]
    fn a_removed_member_cannot_read_what_comes_after_and_a_new_one_reads_everything() {
        let world: Rc<RefCell<World>> = Rc::default();
        let sam = Account::on("sam", AccountKind::E2ee, &world);
        let alex = Account::on("alex", AccountKind::E2ee, &world);
        let kim = Account::on("kim", AccountKind::E2ee, &world);
        let mut laptop = sam.device("laptop");
        World::share(&world, SPACE_ID, "sam", AccountKind::E2ee);
        World::add(&world, SPACE_ID, "alex", SpaceRole::Read);
        laptop.sync();
        let id = laptop.note("Plan", "before alex left");
        laptop.s().move_to_space(Item::Note(id), SPACE_ID).unwrap();
        laptop.sync();
        let mut phone = alex.device("phone");
        phone.sync();
        assert!(phone.view().contains("before alex left"));

        World::remove(&world, SPACE_ID, "alex");
        laptop.sync();
        assert_eq!(
            laptop.s().space_key_epochs(SPACE_ID).unwrap(),
            vec![1, 2],
            "a new key"
        );
        let shared = laptop
            .s()
            .list_notes(NoteScope::All, NoteSort::TitleAsc)
            .unwrap()[0]
            .id;
        laptop
            .s()
            .update_note(
                shared,
                NoteChange {
                    body: Some("after alex left".into()),
                    ..NoteChange::default()
                },
            )
            .unwrap();
        laptop.sync();
        // alex's device kept the old key: the new change does not open with it
        let after = world.borrow().spaces[SPACE_ID]
            .log
            .borrow()
            .changes
            .last()
            .unwrap()
            .clone();
        let bytes = STANDARD.decode(&after.data).unwrap();
        assert_eq!(&bytes[..4], &2u32.to_be_bytes(), "sealed with epoch 2");
        phone.sync();
        assert!(
            !phone.view().contains("after alex left"),
            "and alex is out anyway"
        );

        // kim joins later and reads the whole note, old changes included
        World::add(&world, SPACE_ID, "kim", SpaceRole::Edit);
        laptop.sync();
        let mut tablet = kim.device("tablet");
        tablet.sync();
        assert_eq!(tablet.s().space_key_epochs(SPACE_ID).unwrap(), vec![1, 2]);
        assert!(tablet.view().contains("after alex left"));
        assert_eq!(tablet.s().sync_rejected().unwrap(), 0);
        assert_eq!(
            space_view(&laptop.s(), SPACE_ID),
            space_view(&tablet.s(), SPACE_ID)
        );
    }

    #[test]
    fn a_member_waits_for_the_owner_s_device_before_reading() {
        let world: Rc<RefCell<World>> = Rc::default();
        let sam = Account::on("sam", AccountKind::E2ee, &world);
        let alex = Account::on("alex", AccountKind::E2ee, &world);
        let mut laptop = sam.device("laptop");
        World::share(&world, SPACE_ID, "sam", AccountKind::E2ee);
        laptop.sync();
        let id = laptop.note("Plan", "shared");
        laptop.s().move_to_space(Item::Note(id), SPACE_ID).unwrap();
        laptop.sync();
        // alex is added while sam's devices are away
        World::add(&world, SPACE_ID, "alex", SpaceRole::Edit);
        let mut phone = alex.device("phone");
        phone.sync();
        assert!(!phone.view().contains("shared"), "no key yet");
        assert_eq!(
            phone.s().sync_rejected().unwrap(),
            0,
            "nothing set aside meanwhile"
        );
        laptop.sync();
        phone.sync();
        assert!(phone.view().contains("shared"));
    }

    /// A server listing someone the owner did
    /// not add, or a new key for a member, gets no space key; both wait for
    /// the owner, who can still let them in on purpose.
    #[test]
    fn a_server_listing_a_stranger_or_a_swapped_key_gets_no_space_key() {
        let world: Rc<RefCell<World>> = Rc::default();
        let sam = Account::on("sam", AccountKind::E2ee, &world);
        let _alex = Account::on("alex", AccountKind::E2ee, &world);
        let mut laptop = sam.device("laptop");
        World::share(&world, SPACE_ID, "sam", AccountKind::E2ee);
        World::add(&world, SPACE_ID, "alex", SpaceRole::Edit);
        laptop.sync();

        let mut space = laptop.server.spaces().unwrap().unwrap().spaces[0].clone();
        let server_key = app_e2ee::new_key_pair().public_text();
        let wanted = |user: &str| GrantWanted {
            user_id: user.into(),
            public_key: server_key.clone(),
            epoch: 1,
            name: user.into(),
        };
        space.grants_wanted = vec![wanted("mallory"), wanted("alex")];
        let given = laptop.s().sync_grants_to_give(&space).unwrap();
        assert!(given.iter().all(|g| g.grants.is_empty()), "{given:?}");
        let waiting = laptop.s().space_waiting(SPACE_ID).unwrap();
        assert_eq!(
            waiting
                .iter()
                .map(|w| (w.user_id.as_str(), w.changed))
                .collect::<Vec<_>>(),
            vec![("alex", true), ("mallory", false)]
        );

        // the owner compared and lets mallory in on purpose
        laptop
            .s()
            .space_trust(SPACE_ID, "mallory", Some(&server_key))
            .unwrap();
        let given = laptop.s().sync_grants_to_give(&space).unwrap();
        let to: Vec<&str> = given
            .iter()
            .flat_map(|g| g.grants.iter().map(|grant| grant.user_id.as_str()))
            .collect();
        assert_eq!(to, vec!["mallory"]);
    }

    /// A grant the server makes with a key of
    /// its own, for an epoch far ahead or under a swapped owner key, is not
    /// taken by a member who already reads the space.
    #[test]
    fn a_member_takes_space_keys_from_the_owner_only() {
        let world: Rc<RefCell<World>> = Rc::default();
        let sam = Account::on("sam", AccountKind::E2ee, &world);
        let alex = Account::on("alex", AccountKind::E2ee, &world);
        let mut laptop = sam.device("laptop");
        World::share(&world, SPACE_ID, "sam", AccountKind::E2ee);
        World::add(&world, SPACE_ID, "alex", SpaceRole::Edit);
        let mut phone = alex.device("phone");
        phone.sync();
        laptop.sync();
        phone.sync();
        let id = laptop.note("Plan", "shared");
        laptop.s().move_to_space(Item::Note(id), SPACE_ID).unwrap();
        laptop.sync();
        phone.sync();
        assert!(phone.view().contains("shared"));

        let alex_key = world.borrow().public_keys["alex"].clone();
        let server = app_e2ee::new_key_pair();
        let planted = app_e2ee::new_space_key();
        let mut space = phone.server.spaces().unwrap().unwrap().spaces[0].clone();
        for epoch in [2, u32::MAX] {
            let grant = app_e2ee::grant(SPACE_ID, epoch, &planted, &alex_key, &server).unwrap();
            space.grants.push(SpaceGrant { epoch, grant });
        }
        assert_eq!(phone.s().sync_accept_grants(&space).unwrap(), 0);
        // the server now names its own key as the owner's
        space.owner_key = server.public_text();
        assert_eq!(phone.s().sync_accept_grants(&space).unwrap(), 0);
        assert_eq!(phone.s().space_key_epochs(SPACE_ID).unwrap(), vec![1]);
    }

    #[test]
    fn two_owner_devices_never_seal_with_different_keys() {
        let world: Rc<RefCell<World>> = Rc::default();
        let sam = Account::on("sam", AccountKind::E2ee, &world);
        let mut laptop = sam.device("laptop");
        let mut desk = sam.device("desk");
        World::share(&world, SPACE_ID, "sam", AccountKind::E2ee);
        // both see the space with no key at once: only one key wins
        let listed = laptop.server.spaces().unwrap().unwrap();
        let from_desk = desk.s().sync_grants_to_give(&listed.spaces[0]).unwrap();
        desk.server.give_grants(SPACE_ID, &from_desk[0]).unwrap();
        laptop.sync();
        desk.sync();
        let id = laptop.note("x", "y");
        laptop.s().move_to_space(Item::Note(id), SPACE_ID).unwrap();
        laptop.sync();
        desk.sync();
        same(&laptop, &desk);
        assert_eq!(desk.s().sync_rejected().unwrap(), 0);
    }
}

mod sharing_deep {
    use super::*;
    use crate::model::{NoteScope, NoteSort};
    use crate::store::Item;
    use crate::testing::space_view;

    const S1: &str = "11111111-aaaa-3333-4444-555555555555";
    const S2: &str = "22222222-bbbb-3333-4444-555555555555";

    fn titles(device: &Device) -> Vec<String> {
        let mut titles: Vec<String> = device
            .s()
            .list_notes(NoteScope::All, NoteSort::TitleAsc)
            .unwrap()
            .into_iter()
            .map(|n| format!("{}:{}", n.space_id.is_empty(), n.title))
            .collect();
        titles.sort();
        titles
    }

    /// Sam shares a folder holding "Plan" with Alex, who edits it.
    fn shared(kind: AccountKind) -> (Rc<RefCell<World>>, Device, Device, NoteId) {
        let world: Rc<RefCell<World>> = Rc::default();
        let sam = Account::on("sam", kind, &world);
        let alex = Account::on("alex", kind, &world);
        let mut laptop = sam.device("laptop");
        World::share(&world, S1, "sam", kind);
        World::add(&world, S1, "alex", SpaceRole::Edit);
        laptop.sync();
        let id = laptop.note("Plan", "first");
        laptop.s().move_to_space(Item::Note(id), S1).unwrap();
        laptop.sync();
        let mut phone = alex.device("phone");
        phone.sync();
        let shared = phone
            .s()
            .list_notes(NoteScope::All, NoteSort::TitleAsc)
            .unwrap()[0]
            .id;
        (world, laptop, phone, shared)
    }

    fn edit(device: &Device, id: NoteId, body: &str) {
        device
            .s()
            .update_note(
                id,
                NoteChange {
                    body: Some(body.into()),
                    ..NoteChange::default()
                },
            )
            .unwrap();
    }

    #[test]
    fn a_member_back_in_a_space_writes_to_it_before_reading_it_again() {
        both_kinds(|kind| {
            let (world, mut laptop, mut phone, shared_note) = shared(kind);
            // alex's own changes to the space are on the server
            phone.s().set_pinned(shared_note, true).unwrap();
            phone.sync();
            World::remove(&world, S1, "alex");
            phone.sync();
            World::add(&world, S1, "alex", SpaceRole::Edit);
            if kind == AccountKind::E2ee {
                // the owner hands the new epoch's key first
                laptop.sync();
            }
            // the space is listed again, nothing of it read yet, and alex
            // moves a note into it: written from an empty document
            let listed = phone.server.spaces().unwrap().unwrap();
            phone.s().sync_set_spaces(&listed.spaces).unwrap();
            for space in &listed.spaces {
                phone.s().sync_accept_grants(space).unwrap();
            }
            let mine = phone.note("Alex adds", "to the share");
            phone.s().move_to_space(Item::Note(mine), S1).unwrap();
            phone.sync();
            laptop.sync();
            assert_eq!(
                laptop.s().sync_rejected().unwrap(),
                0,
                "{kind:?}: set aside"
            );
            assert!(laptop.view().contains("to the share"), "{kind:?}");
            assert_eq!(space_view(&laptop.s(), S1), space_view(&phone.s(), S1));
        });
    }

    #[test]
    fn a_member_removed_with_unsent_edits_keeps_them_as_their_own_notes() {
        both_kinds(|kind| {
            let (world, mut laptop, mut phone, shared) = shared(kind);
            edit(&phone, shared, "first, then alex offline");
            World::remove(&world, S1, "alex");
            let round = phone.sync();
            assert_eq!(
                titles(&phone),
                ["true:Plan"],
                "a personal copy, the space gone"
            );
            assert!(phone.view().contains("first, then alex offline"));
            assert_eq!(round.kept, 0, "lost from the list, not refused");
            assert_eq!(phone.s().sync_counts().unwrap().pending, 0);
            laptop.sync();
            assert!(!laptop.view().contains("alex offline"), "sam never got it");
        });
    }

    #[test]
    fn a_member_made_a_reader_with_unsent_edits_keeps_them_and_reads_on() {
        both_kinds(|kind| {
            let (world, mut laptop, mut phone, shared) = shared(kind);
            edit(&phone, shared, "first, alex wrote this");
            // the role changes between the list and the push
            let listed = phone.server.spaces().unwrap().unwrap();
            phone.s().sync_set_spaces(&listed.spaces).unwrap();
            World::add(&world, S1, "alex", SpaceRole::Read);
            let round = phone.sync();
            assert_eq!(round.kept, 1, "{round:?}");
            phone.sync();
            assert_eq!(titles(&phone), ["false:Plan", "true:Plan"]);
            assert!(phone.view().contains("alex wrote this"), "kept");
            assert_eq!(phone.s().sync_counts().unwrap().pending, 0);
            assert_eq!(phone.s().sync_rejected().unwrap(), 0);
            laptop.sync();
            assert_eq!(
                space_view(&laptop.s(), S1),
                space_view(&phone.s(), S1),
                "the space came back whole, as sam has it"
            );
            let note = laptop.s().space_roots(S1).unwrap().notes[0].id;
            edit(&laptop, note, "sam again");
            laptop.sync();
            phone.sync();
            assert_eq!(space_view(&laptop.s(), S1), space_view(&phone.s(), S1));
        });
    }

    #[test]
    fn people_sharing_with_random_edits_roles_and_faults_end_the_same() {
        let runs: u64 = std::env::var("SHARE_FUZZ_RUNS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(12);
        let first_seed: u64 = std::env::var("SHARE_FUZZ_SEED")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);
        for seed in first_seed..first_seed + runs {
            one_run(seed);
        }
    }

    /// A view with tags in lower case: each vault keeps one spelling per
    /// tag, two people may spell the same tag differently.
    fn tags_folded(view: &str) -> String {
        view.lines()
            .map(|line| match line.split_once(" tags=") {
                Some((head, rest)) => {
                    let (tags, tail) = rest.split_once(" trashed=").unwrap_or((rest, ""));
                    format!("{head} tags={} trashed={tail}", tags.to_lowercase())
                }
                None => line.to_owned(),
            })
            .collect::<Vec<_>>()
            .join(
                "
",
            )
    }

    /// Each account's devices alike, each space alike for everyone in it
    /// and empty for the others, nothing left to send or set aside.
    fn check_end(seed: u64, kind: AccountKind, world: &Rc<RefCell<World>>, devices: &[Device]) {
        let account_of = |d: usize| if d < 2 { "sam" } else { "alex" };
        for (i, d) in devices.iter().enumerate() {
            let left: Vec<String> = d
                .s()
                .sync_outbox(10)
                .unwrap()
                .into_iter()
                .map(|o| o.doc_id)
                .collect();
            assert!(
                left.is_empty(),
                "seed {seed} device {i} has to send {left:?}"
            );
            let rejected = d.s().rejected_for_tests();
            assert!(
                rejected.is_empty(),
                "seed {seed} device {i} set aside {rejected:?}"
            );
        }
        for (a, b) in [(0, 1), (2, 3)] {
            assert_eq!(
                devices[a].view(),
                devices[b].view(),
                "seed {seed} ({kind:?}): {} differs",
                account_of(a)
            );
        }
        for space in [S1, S2] {
            let members: Vec<String> = world.borrow().spaces[space]
                .members
                .keys()
                .cloned()
                .collect();
            let holding: Vec<usize> = (0..devices.len())
                .filter(|d| members.iter().any(|m| m == account_of(*d)))
                .collect();
            for d in 0..devices.len() {
                let here = tags_folded(&space_view(&devices[d].s(), space));
                if holding.contains(&d) {
                    assert_eq!(
                        here,
                        tags_folded(&space_view(&devices[holding[0]].s(), space)),
                        "seed {seed} ({kind:?}): {space} differs on device {d}"
                    );
                } else {
                    assert!(here.is_empty(), "seed {seed}: device {d} is not in {space}");
                }
            }
        }
    }

    fn one_run(seed: u64) {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let kind = if seed.is_multiple_of(2) {
            AccountKind::E2ee
        } else {
            AccountKind::Standard
        };
        let world: Rc<RefCell<World>> = Rc::default();
        let sam = Account::on("sam", kind, &world);
        let alex = Account::on("alex", kind, &world);
        let mut devices = vec![
            sam.device("laptop"),
            sam.device("desk"),
            alex.device("phone"),
            alex.device("tablet"),
        ];
        World::share(&world, S1, "sam", kind);
        World::add(&world, S1, "alex", SpaceRole::Edit);
        World::share(&world, S2, "alex", kind);
        World::add(&world, S2, "sam", SpaceRole::Read);
        for (d, space, name) in [(0, S1, "sam's"), (2, S2, "alex's")] {
            devices[d].sync();
            let folder = devices[d].s().create_folder(None, name).unwrap();
            let id = devices[d].note("start", "a");
            devices[d].s().move_note(id, Some(folder.id)).unwrap();
            devices[d]
                .s()
                .move_to_space(Item::Folder(folder.id), space)
                .unwrap();
        }
        for _ in 0..2 {
            for d in &mut devices {
                d.sync();
            }
        }
        for _ in 0..80 {
            let d = rng.below(devices.len());
            match rng.below(12) {
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
                1 | 2 => {
                    let _ = devices[d].sync();
                }
                3 => {
                    // the owners change who is in and how
                    let role = if rng.below(2) == 0 {
                        SpaceRole::Edit
                    } else {
                        SpaceRole::Read
                    };
                    match rng.below(4) {
                        0 => World::add(&world, S1, "alex", role),
                        1 => World::add(&world, S2, "sam", role),
                        2 => World::remove(&world, S1, "alex"),
                        _ => World::remove(&world, S2, "sam"),
                    }
                }
                4 => {
                    // a note goes into one of the shared spaces
                    let notes = devices[d]
                        .s()
                        .list_notes(NoteScope::All, NoteSort::TitleAsc)
                        .unwrap();
                    let space = if rng.below(2) == 0 { S1 } else { S2 };
                    if let Some(note) = rng.pick(&notes.iter().map(|n| n.id).collect::<Vec<_>>()) {
                        let _ = devices[d].s().move_to_space(Item::Note(note), space);
                    }
                }
                _ => random_change(&mut devices[d].s(), &mut rng),
            }
        }
        // owners first, so e2ee members find their keys
        for _ in 0..3 {
            for d in &mut devices {
                d.sync();
            }
        }
        check_end(seed, kind, &world, &devices);
    }
}

mod files_deep {
    use super::*;
    use crate::blobs::BlobStore;
    use crate::model::{NoteScope, NoteSort};
    use crate::store::{FileKeying, Item};
    use crate::sync::files::transfer;
    use crate::testing::space_view;

    const S1: &str = "33333333-cccc-3333-4444-555555555555";

    /// A device with its files folder.
    struct Holder {
        device: Device,
        blobs: BlobStore,
        account: &'static str,
        _dir: tempfile::TempDir,
    }

    thread_local! {
        /// The time every device of a run sees: the same run, the same steps.
        static CLOCK: std::sync::Arc<crate::clock::ManualClock> =
            std::sync::Arc::new(crate::clock::ManualClock::new(1_800_000_000_000));
    }

    fn holder(account: &Account, name: &str, who: &'static str) -> Holder {
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
            account: who,
            _dir: dir,
        }
    }

    /// A notes round then a files round, the faults on both.
    fn round(h: &mut Holder, faults: Faults) {
        h.device.server.faults = faults;
        let notes = run(&h.device.store, &mut h.device.server);
        if notes.is_ok() || faults.lose_answer {
            let _ = transfer(&h.device.store, &h.blobs, &mut h.device.server, u64::MAX);
        }
        h.device.server.faults = Faults::default();
    }

    fn clean(h: &mut Holder) {
        round(h, Faults::default());
        transfer(&h.device.store, &h.blobs, &mut h.device.server, u64::MAX).unwrap();
    }

    fn notes(h: &Holder) -> Vec<(NoteId, String)> {
        h.device
            .s()
            .list_notes(NoteScope::All, NoteSort::TitleAsc)
            .unwrap()
            .into_iter()
            .map(|n| (n.id, n.space_id))
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

    #[expect(
        clippy::too_many_lines,
        reason = "every end condition, with what to show when one fails"
    )]
    fn check_end(
        seed: u64,
        kind: AccountKind,
        world: &Rc<RefCell<World>>,
        sam: &Account,
        alex: &Account,
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
        if va != vb {
            let only = |x: &str, y: &str| {
                x.lines()
                    .filter(|l| !y.lines().any(|m| m == *l))
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
                    .join(
                        "
",
                    )
            };
            let note = only(&va, &vb)
                .split("note=")
                .nth(1)
                .map(|n| n[..36].to_owned())
                .unwrap_or_default();
            let waiting = (
                holders[0].device.s().waiting_for_tests(),
                holders[1].device.s().waiting_for_tests(),
            );
            let docs = (
                holders[0].device.s().debug_files_for_tests(S1, &note),
                holders[1].device.s().debug_files_for_tests(S1, &note),
            );
            let changes = world.borrow().spaces[S1].log.borrow().changes.len();
            panic!(
                "seed {seed} ({kind:?}): sam's devices differ (waiting {waiting:?}, server S1 changes {changes})
laptop only:
{}
phone only:
{}
laptop doc {}
phone doc {}",
                only(&va, &vb),
                only(&vb, &va),
                docs.0,
                docs.1,
            );
        }
        if world.borrow().spaces[S1].members.contains_key("alex") {
            // two people may spell a tag differently (accepted, see the
            // sharing tests): compared without case
            let (va, vb) = (
                space_view(&holders[0].device.s(), S1).to_lowercase(),
                space_view(&holders[2].device.s(), S1).to_lowercase(),
            );
            if va != vb {
                let only = |x: &str, y: &str| {
                    x.lines()
                        .filter(|l| !y.lines().any(|m| m == *l))
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                        .join(
                            "
",
                        )
                };
                // the server's log of the space document, as a reader sees it
                let server = loro::LoroDoc::new();
                for c in &world.borrow().spaces[S1].log.borrow().changes {
                    if c.doc == "space" {
                        let _ = server.import(&STANDARD.decode(&c.data).unwrap());
                    }
                }
                let docs = (
                    holders[0].device.s().debug_files_for_tests(S1, "x"),
                    holders[2].device.s().debug_files_for_tests(S1, "x"),
                );
                panic!(
                    "seed {seed} ({kind:?}): the shared space differs
sam only:
{}
alex only:
{}
sam {}
alex {}
server vv {:?}",
                    only(&va, &vb),
                    only(&vb, &va),
                    docs.0,
                    docs.1,
                    server.oplog_vv()
                );
            }
        }
        let holding = |sha: &str| -> Vec<usize> {
            holders
                .iter()
                .enumerate()
                .filter(|(_, h)| h.blobs.has(sha).unwrap())
                .map(|(i, _)| i)
                .collect()
        };
        for (i, h) in holders.iter().enumerate() {
            let uses = h.device.s().files_in_use().unwrap();
            for file in uses.iter().filter(|f| made.contains_key(&f.sha256)) {
                // every file a note here uses is here, and is what was attached
                let here = h.blobs.get(&file.sha256).unwrap();
                assert!(
                    here.as_deref() == Some(made[&file.sha256].as_slice()),
                    "seed {seed} ({kind:?}) device {i}: {} missing or wrong, held by {:?}, server {:?}, {:?} space {:?} epoch {:?}, keys {:?}",
                    file.sha256,
                    holding(&file.sha256),
                    {
                        let keying = h.device.s().file_keying(&file.space, file.epoch).unwrap();
                        let address = match &keying {
                            FileKeying::Plain => file.sha256.clone(),
                            FileKeying::Sealed(key) => app_e2ee::file_address(key, &file.sha256),
                            FileKeying::Waiting => String::new(),
                        };
                        let log = if file.space.is_empty() {
                            if h.account == "sam" {
                                sam.log.clone()
                            } else {
                                alex.log.clone()
                            }
                        } else {
                            world.borrow().spaces[&file.space].log.clone()
                        };
                        let log = log.borrow();
                        log.files
                            .get(&address)
                            .map(|f| (f.complete, f.released, f.data.len()))
                    },
                    here.map(|h| h.len()),
                    file.space,
                    file.epoch,
                    h.device.s().space_key_epochs(S1).unwrap(),
                );
                // and the server holds it for the space, not let go
                let keying = h.device.s().file_keying(&file.space, file.epoch).unwrap();
                let address = match &keying {
                    FileKeying::Plain => file.sha256.clone(),
                    FileKeying::Sealed(key) => app_e2ee::file_address(key, &file.sha256),
                    FileKeying::Waiting => panic!("seed {seed}: no key for a file"),
                };
                let log = if file.space.is_empty() {
                    if h.account == "sam" {
                        sam.log.clone()
                    } else {
                        alex.log.clone()
                    }
                } else {
                    world.borrow().spaces[&file.space].log.clone()
                };
                let log = log.borrow();
                let stored = log.files.get(&address).unwrap_or_else(|| {
                    panic!("seed {seed} ({kind:?}) device {i}: not on the server")
                });
                assert!(
                    stored.complete && !stored.released,
                    "seed {seed} device {i}: let go {} {} {:?}",
                    &file.sha256[..6],
                    &address[..8],
                    file.space
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
    fn people_attaching_moving_and_removing_files_with_faults_end_with_every_file() {
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
            let world: Rc<RefCell<World>> = Rc::default();
            let sam = Account::on("sam", kind, &world);
            let alex = Account::on("alex", kind, &world);
            let mut holders = vec![
                holder(&sam, "laptop", "sam"),
                holder(&sam, "phone", "sam"),
                holder(&alex, "tablet", "alex"),
            ];
            World::share(&world, S1, "sam", kind);
            World::add(&world, S1, "alex", SpaceRole::Edit);
            for h in &mut holders {
                clean(h);
            }
            for h in &mut holders {
                clean(h);
            }
            let mut made: HashMap<String, Vec<u8>> = HashMap::new();
            for _ in 0..70 {
                CLOCK.with(|c| c.advance(i64::try_from(rng.below(5)).unwrap()));
                let d = rng.below(holders.len());
                let op = rng.below(12);
                if op == 8 {
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
                        let Some(&(note, _)) = list.get(rng.below(list.len().max(1))) else {
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
                            .flat_map(|(n, _)| h.device.s().list_attachments(n).unwrap())
                            .collect();
                        if let Some(a) = files.get(rng.below(files.len().max(1))) {
                            let _ = h.device.s().remove_attachment(a.id);
                        }
                    }
                    7 => {
                        let list = notes(h);
                        if let Some((note, space)) = list.get(rng.below(list.len().max(1))) {
                            let to = if space.is_empty() { S1 } else { "" };
                            let _ = h.device.s().move_to_space(Item::Note(*note), to);
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
            // until a whole pass sends nothing: what a round writes (an
            // epoch, a name settled) goes with the next one
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
            check_end(seed, kind, &world, &sam, &alex, &holders, &made);
        }
    }
}
