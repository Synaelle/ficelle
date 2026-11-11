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
use crate::wire::{self, Incoming, Msg};
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

    // a write on b reaches a while both stay connected, by itself
    let id = note(&hb, "From the phone", "typed on the bus");
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
fn wrong_codes_sent_at_once_still_get_three_tries() {
    let (a, _ha) = node("Desktop");
    let shown = a.start_pairing().unwrap();
    let wrong = if shown.code == "000000" {
        "111111"
    } else {
        "000000"
    };
    let tryers: Vec<_> = (0..8).map(|i| node(&format!("Phone {i}"))).collect();
    std::thread::scope(|scope| {
        for (b, _) in &tryers {
            scope.spawn(|| b.pair_with_code(wrong, Some(&address(&a))));
        }
    });
    // the ones arriving while a try is checked are turned away unchecked
    let checked = a.shared.codes_checked();
    assert!(checked <= 3, "{checked} codes were checked");
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
        port: None,
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

/// A node that never uses discovery, on a port of its own that stays the
/// same when it starts again.
fn hidden_node(name: &str) -> (Node<TestHost>, Arc<TestHost>, u16) {
    let host = TestHost::new(name);
    let me = Me {
        platform: "linux".into(),
        version: "0.6.0-test".into(),
    };
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let node = Node::with_port(host.clone(), me, "", port);
    node.set_wanted(true);
    (node, host, port)
}

fn kept_address(host: &TestHost) -> Option<String> {
    host.s(|s, _| s.peer_devices()).unwrap()[0].address.clone()
}

#[test]
fn a_device_discovery_cannot_see_is_reached_at_the_address_it_answered_on() {
    let (a, ha, _) = hidden_node("Desktop");
    let (b, hb, port_b) = hidden_node("Laptop");
    let shown = a.start_pairing().unwrap();
    b.pair_with_code(&shown.code, Some(&address(&a))).unwrap();
    // b dialled a: a learned b's address from b's hello, with b's port
    until("a kept b's address", || {
        kept_address(&ha).as_deref() == Some(&format!("127.0.0.1:{port_b}"))
    });
    // b's own way back to a is broken, so only a dialling b can join them
    let b_view_of_a = hb.s(|s, _| s.peer_devices()).unwrap()[0].clone();
    hb.s(|s, _| {
        s.peer_seen(
            &b_view_of_a.key,
            &b_view_of_a.name,
            &b_view_of_a.platform,
            &b_view_of_a.version,
            Some("127.0.0.1:1"),
            false,
        )
    })
    .unwrap();
    a.set_wanted(false);
    until("a stopped", || !a.status().running);
    let id = note(&ha, "Written while apart", "no multicast here");
    a.set_wanted(true);
    until("b has a's note, a having dialled b", || {
        has_note(&hb, "Written while apart")
    });
    assert!(hb.s(|s, _| s.note(id)).is_ok());
}

#[test]
fn a_device_out_of_reach_the_way_a_firewall_does_it_shows_after_a_minute() {
    let (a, ha) = node("Desktop");
    let laptop = [7u8; 32];
    let phone = [8u8; 32];
    ha.s(|s, _| {
        s.peer_pair(&laptop, "Work laptop", "linux", "0.6.0")?;
        s.peer_pair(&phone, "Pixel 8", "android", "0.6.0")
    })
    .unwrap();
    // running, as it is while it dials them
    a.refresh();
    until("a runs", || a.status().running);
    let shared = &a.shared;
    let kept: std::net::SocketAddr = "192.0.2.1:47822".parse().unwrap();
    let refused = std::io::Error::from(std::io::ErrorKind::ConnectionRefused);
    let silent = std::io::Error::from(std::io::ErrorKind::TimedOut);
    // refused at a kept address: the app is just closed there
    shared.failed_like_a_firewall(kept, laptop, &refused);
    assert!(!shared.unreachable_for(laptop, Duration::ZERO));
    // a phone that does not answer at a kept address is in a pocket
    shared.failed_like_a_firewall(kept, phone, &silent);
    assert!(!shared.unreachable_for(phone, Duration::ZERO));
    // no answer at all from a computer: a firewall dropping the connection
    shared.failed_like_a_firewall(kept, laptop, &silent);
    assert!(a.status().blocked.is_empty(), "not before a minute");
    assert!(shared.unreachable_for(laptop, crate::node::BLOCKED_AFTER));
    assert_eq!(a.status().blocked, vec!["07".repeat(32)]);
    // the phone announced by discovery but refusing: the app is open there
    let announced: std::net::SocketAddr = "192.0.2.2:47822".parse().unwrap();
    shared.announce(announced);
    shared.failed_like_a_firewall(announced, phone, &refused);
    assert!(shared.unreachable_for(phone, crate::node::BLOCKED_AFTER));
    assert_eq!(a.status().blocked.len(), 2);
    // forgetting a device takes its hint away
    ha.s(|s, _| s.peer_forget(&phone)).unwrap();
    a.forget(&phone);
    assert_eq!(a.status().blocked.len(), 1);
}

type Device = (Node<TestHost>, Arc<TestHost>);

/// Two nodes without discovery, paired, `b` having dialled `a`.
fn paired() -> (Device, Device) {
    let (a, ha, _) = hidden_node("Desktop");
    let (b, hb, _) = hidden_node("Phone");
    let shown = a.start_pairing().unwrap();
    b.pair_with_code(&shown.code, Some(&address(&a))).unwrap();
    until("both say synced", || {
        let synced = |n: &Node<TestHost>| n.status().live.values().any(|l| *l == Live::Synced);
        synced(&a) && synced(&b)
    });
    ((a, ha), (b, hb))
}

fn body(host: &TestHost, id: app_core::id::NoteId) -> String {
    host.s(|s, _| s.note(id))
        .map(|n| n.body)
        .unwrap_or_default()
}

fn edit(host: &TestHost, id: app_core::id::NoteId, body: &str) {
    host.s(|s, _| {
        s.update_note(
            id,
            NoteChange {
                body: Some(body.into()),
                ..NoteChange::default()
            },
        )
    })
    .unwrap();
}

#[test]
fn the_first_change_ever_made_on_a_device_reaches_the_other_by_itself() {
    let ((_a, ha), (_b, hb)) = paired();
    // every note so far was written on a: b never changed anything
    let id = note(&ha, "Shopping", "milk");
    until("b has the note", || has_note(&hb, "Shopping"));
    assert_eq!(hb.s(|s, _| s.peer_write_mark()).unwrap(), None);
    edit(&hb, id, "milk, bread");
    until("a has b's first edit, a doing nothing", || {
        body(&ha, id) == "milk, bread"
    });
    // and it goes on both ways, each change told once to the app
    let told = ha.changed.load(Ordering::SeqCst);
    edit(&hb, id, "milk, bread, eggs");
    until("a has b's second edit", || {
        body(&ha, id) == "milk, bread, eggs"
    });
    edit(&ha, id, "milk, bread, eggs, salt");
    until("b has a's edit", || {
        body(&hb, id) == "milk, bread, eggs, salt"
    });
    assert_eq!(ha.changed.load(Ordering::SeqCst), told + 1);
}

#[test]
fn a_change_taken_in_does_not_come_back_and_nothing_runs_when_idle() {
    let ((a, ha), (b, hb)) = paired();
    let id = note(&ha, "Shopping", "milk");
    until("b has the note", || has_note(&hb, "Shopping"));
    edit(&hb, id, "milk, bread");
    until("a has b's edit", || body(&ha, id) == "milk, bread");
    // let any round still running end
    std::thread::sleep(Duration::from_secs(2));
    let synced_at = |h: &TestHost| h.s(|s, _| s.peer_devices()).unwrap()[0].last_sync_at;
    let before = (
        ha.changed.load(Ordering::SeqCst),
        hb.changed.load(Ordering::SeqCst),
        ha.s(|s, _| s.peer_write_mark()).unwrap(),
        hb.s(|s, _| s.peer_write_mark()).unwrap(),
    );
    // a round would note a later time
    let rounds = (synced_at(&ha), synced_at(&hb));
    std::thread::sleep(Duration::from_secs(3));
    let after = (
        ha.changed.load(Ordering::SeqCst),
        hb.changed.load(Ordering::SeqCst),
        ha.s(|s, _| s.peer_write_mark()).unwrap(),
        hb.s(|s, _| s.peer_write_mark()).unwrap(),
    );
    assert_eq!(before, after, "something went on while idle");
    assert_eq!(
        rounds,
        (synced_at(&ha), synced_at(&hb)),
        "a round while idle"
    );
    let synced = |n: &Node<TestHost>| n.status().live.values().all(|l| *l == Live::Synced);
    assert!(synced(&a) && synced(&b));
}

#[test]
fn edits_made_on_both_devices_at_once_merge_on_both() {
    let ((_a, ha), (_b, hb)) = paired();
    let id = note(&ha, "Trip", "Pack light.\n\nLeave on Monday.");
    until("b has the note", || has_note(&hb, "Trip"));
    edit(&ha, id, "Pack light, and a raincoat.\n\nLeave on Monday.");
    edit(&hb, id, "Pack light.\n\nLeave on Monday at seven.");
    let merged = "Pack light, and a raincoat.\n\nLeave on Monday at seven.";
    until("both hold both edits", || {
        body(&ha, id) == merged && body(&hb, id) == merged
    });
}

#[test]
fn a_change_reaches_a_device_that_only_a_third_one_connects_to() {
    let (a, ha, _) = hidden_node("Desktop");
    let (b, hb, _) = hidden_node("Laptop");
    let (c, hc, _) = hidden_node("Phone");
    // b pairs with a, then c with b: without discovery a and c learn of
    // each other but have no address to meet at
    let shown = a.start_pairing().unwrap();
    b.pair_with_code(&shown.code, Some(&address(&a))).unwrap();
    let shown = b.start_pairing().unwrap();
    c.pair_with_code(&shown.code, Some(&address(&b))).unwrap();
    let id = note(&ha, "Shopping", "milk");
    until("c has a's note through b", || has_note(&hc, "Shopping"));
    edit(&hc, id, "milk, bread");
    until("a has c's edit through b", || {
        body(&ha, id) == "milk, bread"
    });
    assert_eq!(body(&hb, id), "milk, bread");
}

/// Pairs `node` with a phone made by hand, whose key sorts after the
/// node's own: of two connections the phone dials, the node keeps the
/// first. The phone's secret.
fn pair_by_hand(node: &Node<TestHost>, host: &TestHost) -> [u8; 32] {
    // showing a code makes the node's own key
    node.start_pairing().unwrap();
    node.cancel_pairing();
    let own = channel::public_key(&host.s(|s, _| s.peer_secret()).unwrap().unwrap()).unwrap();
    let (secret, key) = loop {
        let secret = channel::new_secret().unwrap();
        let key = channel::public_key(&secret).unwrap();
        if key > own {
            break (secret, key);
        }
    };
    host.s(|s, _| s.peer_pair(&key, "Phone", "android", "0.6.0-test"))
        .unwrap();
    node.refresh();
    until("the node runs", || node.status().running);
    secret
}

/// A paired device made by hand: dials `node` and says hello, as the
/// app does, then is whatever the test makes of the channel.
fn dial_by_hand(node: &Node<TestHost>, host: &TestHost, secret: &[u8; 32]) -> channel::Channel {
    let key = channel::public_key(&host.s(|s, _| s.peer_secret()).unwrap().unwrap()).unwrap();
    let stream = TcpStream::connect(address(node)).unwrap();
    let mut ch = channel::dial_sync(stream, secret, &key).unwrap();
    ch.send(&wire::encode(&Msg::Hello {
        name: "Phone".into(),
        platform: "android".into(),
        version: "0.6.0-test".into(),
        min_protocol: app_protocol::local::MIN_PROTOCOL,
        protocol: app_protocol::local::PROTOCOL,
        format: app_protocol::local::DATA_FORMAT,
        port: None,
    }))
    .unwrap();
    ch
}

/// Whether the node goes on talking on this connection: it sends its
/// devices, and answers a ping.
fn answers(ch: &mut channel::Channel) -> bool {
    if ch.send(&wire::encode(&Msg::Ping)).is_err() {
        return false;
    }
    let end = Instant::now() + Duration::from_secs(3);
    while Instant::now() < end {
        match ch.poll() {
            Ok(Some(message)) => {
                if matches!(wire::decode(&message), Ok(Incoming::Msg(Msg::Pong))) {
                    return true;
                }
            }
            Ok(None) => {}
            Err(_) => return false,
        }
    }
    false
}

#[test]
fn a_connection_that_died_without_a_word_gives_way_when_the_device_dials_again() {
    let (a, ha) = node("Desktop");
    let secret = pair_by_hand(&a, &ha);
    // the first connection goes silent, as when the phone leaves the
    // network: it stays open and never reads or answers again
    let silent = dial_by_hand(&a, &ha, &secret);
    until("a has a session with the phone", || {
        !a.status().live.is_empty()
    });
    // the phone is back and dials again: turned away at first, then let in
    // once the old connection failed to answer
    until("a talks to the phone on a new connection", || {
        answers(&mut dial_by_hand(&a, &ha, &secret))
    });
    drop(silent);
}

#[test]
fn a_connection_that_still_answers_is_kept_when_the_device_dials_twice() {
    let (a, ha) = node("Desktop");
    let secret = pair_by_hand(&a, &ha);
    let mut first = dial_by_hand(&a, &ha, &secret);
    assert!(answers(&mut first));
    // quiet for a while, as two devices with nothing to say are
    std::thread::sleep(Duration::from_secs(6));
    // a second one is turned away, and the first is asked whether it is
    // still there
    let mut second = dial_by_hand(&a, &ha, &secret);
    assert!(!answers(&mut second));
    until("a asks the first connection", || {
        matches!(
            first.poll().map(|m| m.map(|m| wire::decode(&m))),
            Ok(Some(Ok(Incoming::Msg(Msg::Ping))))
        )
    });
    first.send(&wire::encode(&Msg::Pong)).unwrap();
    // it said so, and stays past the time it had to answer
    std::thread::sleep(Duration::from_secs(6));
    assert!(answers(&mut first), "the live connection was dropped");
}
