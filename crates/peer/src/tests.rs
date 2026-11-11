// SPDX-License-Identifier: AGPL-3.0-or-later

//! Two or three nodes in one process, on loopback, each with a vault of its
//! own in a scratch folder.

use std::net::TcpStream;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use app_core::blobs::{BlobStore, sha256_hex};
use app_core::store::{NewNote, NoteChange, Store};

use crate::session::{Hello, mismatch};
use crate::{Error, Host, Live, Me, Node, channel};

struct TestHost {
    vault: Mutex<(Store, BlobStore)>,
    changed: AtomicU32,
    files: AtomicU32,
    name: String,
    _dir: tempfile::TempDir,
}

impl TestHost {
    fn new(name: &str) -> Arc<Self> {
        let dir = tempfile::tempdir().unwrap();
        let conn = app_core::db::open(&dir.path().join("vault.db"), None).unwrap();
        let blobs = BlobStore::new(dir.path().join("blobs"), None);
        Arc::new(Self {
            vault: Mutex::new((Store::new(conn), blobs)),
            changed: AtomicU32::new(0),
            files: AtomicU32::new(0),
            name: name.to_owned(),
            _dir: dir,
        })
    }

    fn s<T>(&self, f: impl FnOnce(&mut Store, &BlobStore) -> T) -> T {
        let mut vault = self.vault.lock().unwrap();
        let (store, blobs) = &mut *vault;
        f(store, blobs)
    }
}

impl Host for TestHost {
    fn with<T>(
        &self,
        f: impl FnOnce(&mut Store, &BlobStore) -> app_core::Result<T>,
    ) -> Option<app_core::Result<T>> {
        Some(self.s(f))
    }
    fn changed(&self) {
        self.changed.fetch_add(1, Ordering::SeqCst);
    }
    fn files_arrived(&self) {
        self.files.fetch_add(1, Ordering::SeqCst);
    }
    fn status_changed(&self) {}
    fn fetch_up_to(&self) -> u64 {
        25 * 1024 * 1024
    }
    fn default_name(&self) -> String {
        self.name.clone()
    }
}

fn node(name: &str) -> (Node<TestHost>, Arc<TestHost>) {
    let host = TestHost::new(name);
    let me = Me {
        platform: "linux".into(),
        version: "0.6.0-test".into(),
    };
    // port 0: any free one, the tests find it in the status
    let node = Node::with_port(host.clone(), me, "_ficelle-test._tcp.local.", 0);
    node.set_wanted(true);
    (node, host)
}

fn until(what: &str, mut ok: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(20);
    while Instant::now() < end {
        if ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out: {what}");
}

fn address(node: &Node<TestHost>) -> String {
    format!("127.0.0.1:{}", node.status().port.expect("listening"))
}

fn note(host: &TestHost, title: &str, body: &str) -> app_core::id::NoteId {
    host.s(|s, _| {
        s.create_note(&NewNote {
            title: title.into(),
            body: body.into(),
            ..NewNote::default()
        })
    })
    .unwrap()
    .id
}

fn has_note(host: &TestHost, title: &str) -> bool {
    host.s(|s, _| s.search(title, 5))
        .unwrap()
        .iter()
        .any(|n| n.title == title)
}

#[test]
fn pairing_by_address_then_notes_and_files_go_both_ways() {
    let (a, ha) = node("Desktop");
    let (b, hb) = node("Phone");
    note(&ha, "Garden plan", "tomatoes along the fence");
    let bytes = b"a small scan".to_vec();
    let sha = sha256_hex(&bytes);
    ha.s(|s, blobs| {
        blobs.put(&bytes)?;
        let receipt = s.create_note(&NewNote {
            title: "Receipt".into(),
            body: "scan attached".into(),
            ..NewNote::default()
        })?;
        s.add_attachment(receipt.id, "scan.pdf", "application/pdf", 12, &sha)
    })
    .unwrap();

    let shown = a.start_pairing().unwrap();
    assert_eq!(shown.code.len(), 6);
    let name = b.pair_with_code(&shown.code, Some(&address(&a))).unwrap();
    assert_eq!(name, "Desktop");
    until("a shows it paired", || {
        a.status().pairing.and_then(|p| p.paired).as_deref() == Some("Phone")
    });
    until("b has a's notes", || has_note(&hb, "Garden plan"));
    until("b has the file", || {
        hb.s(|_, blobs| blobs.has(&sha)).unwrap()
    });
    assert!(hb.changed.load(Ordering::SeqCst) > 0);

    // both pinned the other
    let keys = |h: &TestHost| h.s(|s, _| s.peer_devices()).unwrap();
    assert_eq!(keys(&ha).len(), 1);
    assert_eq!(keys(&hb)[0].name, "Desktop");

    // a write on b reaches a while both stay connected
    let id = note(&hb, "From the phone", "typed on the bus");
    b.kick();
    until("a has b's note", || has_note(&ha, "From the phone"));
    ha.s(|s, _| {
        s.update_note(
            id,
            NoteChange {
                body: Some("typed on the bus, then edited at home".into()),
                ..NoteChange::default()
            },
        )
    })
    .unwrap();
    a.kick();
    until("b has a's edit", || {
        hb.s(|s, _| s.note(id)).unwrap().body.contains("at home")
    });
    until("both say synced", || {
        let synced = |n: &Node<TestHost>| n.status().live.values().any(|l| *l == Live::Synced);
        synced(&a) && synced(&b)
    });

    // b goes away, a writes, b comes back: a new connection, by key
    b.set_wanted(false);
    until("b stopped", || !b.status().running);
    note(&ha, "Written while away", "the phone was asleep");
    b.set_wanted(true);
    until("b caught up after coming back", || {
        has_note(&hb, "Written while away")
    });
}

#[test]
fn three_wrong_codes_and_the_code_is_gone() {
    let (a, _ha) = node("Desktop");
    let (b, _hb) = node("Phone");
    let shown = a.start_pairing().unwrap();
    let wrong = if shown.code == "000000" {
        "111111"
    } else {
        "000000"
    };
    for _ in 0..3 {
        let err = b.pair_with_code(wrong, Some(&address(&a))).unwrap_err();
        assert!(matches!(err, Error::WrongCode), "{err}");
    }
    until("the code is gone", || {
        a.status().pairing.and_then(|p| p.failed) == Some("tooManyTries")
    });
    // even the right one does not work now
    assert!(b.pair_with_code(&shown.code, Some(&address(&a))).is_err());
}

#[test]
fn a_device_not_paired_gets_nothing() {
    let (a, ha) = node("Desktop");
    let (b, _hb) = node("Phone");
    note(&ha, "Private", "not for strangers");
    // a and b pair, so a listens
    let shown = a.start_pairing().unwrap();
    b.pair_with_code(&shown.code, Some(&address(&a))).unwrap();
    // a stranger with a key of its own dials in, even knowing a's key
    let stranger = channel::new_secret().unwrap();
    let a_key = channel::public_key(&ha.s(|s, _| s.peer_secret()).unwrap().unwrap()).unwrap();
    let stream = TcpStream::connect(address(&a)).unwrap();
    assert!(
        channel::dial_sync(stream, &stranger, &a_key).is_err(),
        "a answered a stranger"
    );
    // and one that does not know a's key gets nothing back either
    let stream = TcpStream::connect(address(&a)).unwrap();
    let guess = channel::public_key(&channel::new_secret().unwrap()).unwrap();
    assert!(channel::dial_sync(stream, &stranger, &guess).is_err());
    assert_eq!(ha.s(|s, _| s.peer_devices()).unwrap().len(), 1);
}

#[test]
fn pairing_is_refused_while_a_server_is_linked() {
    let (a, ha) = node("Desktop");
    ha.s(|s, _| {
        s.sync_set_link(&app_core::store::NewLink {
            server: "https://notes.example.org".into(),
            account_id: "account".into(),
            username: "sam".into(),
            kind: app_protocol::e2ee::AccountKind::Standard,
            master_key: None,
            device_id: "device".into(),
            tokens: app_core::store::Tokens {
                access: "a".into(),
                access_expires_at: 0,
                refresh: "r".into(),
                refresh_expires_at: 0,
            },
        })
    })
    .unwrap();
    assert!(a.start_pairing().is_err());
    assert!(a.pair_with_code("123456", Some("127.0.0.1:9")).is_err());
}

#[test]
fn devices_on_different_data_formats_say_who_updates() {
    let hello = |format, protocol| Hello {
        name: "Other".into(),
        platform: "linux".into(),
        version: "9.9.9".into(),
        min_protocol: 1,
        protocol,
        format,
    };
    let now = app_protocol::local::DATA_FORMAT;
    assert_eq!(mismatch(&hello(now, app_protocol::local::PROTOCOL)), None);
    assert!(matches!(
        mismatch(&hello(now + 1, app_protocol::local::PROTOCOL)),
        Some(Live::UpdateThis { .. })
    ));
    assert!(matches!(
        mismatch(&hello(now - 1, app_protocol::local::PROTOCOL)),
        Some(Live::NeedsUpdate { .. })
    ));
}

#[test]
fn a_large_message_crosses_in_many_frames() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let sa = channel::new_secret().unwrap();
    let sb = channel::new_secret().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut first = [0u8; 1];
        std::io::Read::read_exact(&mut stream, &mut first).unwrap();
        let mut ch = channel::answer_sync(stream, &sb, |_| true).unwrap();
        ch.recv().unwrap()
    });
    let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let mut ch = channel::dial_sync(stream, &sa, &channel::public_key(&sb).unwrap()).unwrap();
    let big: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
    ch.send(&big).unwrap();
    assert_eq!(server.join().unwrap(), big);
}
