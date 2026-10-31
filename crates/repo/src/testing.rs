// SPDX-License-Identifier: AGPL-3.0-or-later

//! Checks any target must pass, for the targets' tests here and in the
//! server (feature `testing`).

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use crate::{Error, Item, KdfParams, Repo, Snapshot, Target, Writer};

/// Bytes that do not repeat, the same for the same seed.
pub fn noise(seed: u64, len: usize) -> Vec<u8> {
    let mut x = seed * 2 + 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            // the low byte, which always fits
            (x & 0xff).to_le_bytes()[0]
        })
        .collect()
}

/// A name no other test run uses.
pub fn unique() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("t{nanos}")
}

/// Every call of a target, in a folder of its own: missing files, writes,
/// reads whole and in part, listings, replacing, removing.
///
/// # Panics
///
/// When the target does not behave.
pub fn target_calls(target: &dyn Target) {
    let base = unique();
    let path = |p: &str| format!("{base}/{p}");
    assert!(matches!(target.get(&path("none")), Err(Error::NotFound(_))));
    assert!(target.list(&format!("{base}/")).unwrap().is_empty());
    let big = noise(1, 3 * 1024 * 1024 + 17);
    target.put(&path("data/ab/one"), &big).unwrap();
    target.put(&path("data/cd/two"), b"two").unwrap();
    target.put(&path("index/x"), b"").unwrap();
    assert_eq!(target.get(&path("data/ab/one")).unwrap(), big);
    assert_eq!(target.get(&path("index/x")).unwrap(), b"");
    assert_eq!(
        target
            .get_range(&path("data/ab/one"), 1_000_000, 4096)
            .unwrap(),
        big[1_000_000..1_004_096]
    );
    let listed: Vec<(String, u64)> = target
        .list(&format!("{base}/"))
        .unwrap()
        .into_iter()
        .map(|l| (l.path, l.size))
        .collect();
    assert_eq!(
        listed,
        vec![
            (path("data/ab/one"), big.len() as u64),
            (path("data/cd/two"), 3),
            (path("index/x"), 0),
        ]
    );
    assert_eq!(target.list(&path("data/")).unwrap().len(), 2);
    // replaced whole
    target.put(&path("data/cd/two"), b"second").unwrap();
    assert_eq!(target.get(&path("data/cd/two")).unwrap(), b"second");
    for p in ["data/ab/one", "data/cd/two", "index/x", "not/there"] {
        target.delete(&path(p)).unwrap();
    }
    assert!(target.list(&format!("{base}/")).unwrap().is_empty());
}

/// A repository's life on a target: five snapshots (each run storing only
/// its new content), reopened, three forgotten, pruned, checked whole, read
/// back, a wrong password refused.
///
/// # Panics
///
/// When something does not hold.
pub fn repository_life(target: &Arc<dyn Target>) {
    let repo = Repo::init(target.clone(), "pw", KdfParams::INSECURE_FOR_TESTS, 0).unwrap();
    let mut ids = Vec::new();
    for day in 0..5_u64 {
        let index = repo.index().unwrap();
        let mut writer = Writer::new(&repo, &index);
        let mut items = Vec::new();
        for (name, bytes) in [
            ("kept", noise(9, 2_000_000)),
            ("daily", noise(day + 1, 600_000)),
        ] {
            let (objects, size) = writer.add_stream(&mut bytes.as_slice()).unwrap();
            items.push(Item {
                name: name.into(),
                size,
                objects,
            });
        }
        let stats = writer.finish().unwrap();
        if day > 0 {
            assert!(
                stats.new_bytes <= 700_000,
                "only the day's content is new: {stats:?}"
            );
        }
        let mut snapshot = Snapshot {
            id: String::new(),
            time: i64::try_from(day).unwrap(),
            host: "test".into(),
            kind: "test".into(),
            app_version: "0".into(),
            items,
            counts: BTreeMap::new(),
        };
        repo.save_snapshot(&mut snapshot).unwrap();
        ids.push(snapshot.id);
    }
    let opened = Repo::open(target.clone(), "pw").unwrap();
    assert_eq!(opened.snapshots().unwrap().len(), 5);
    let gone: HashSet<String> = ids[..3].iter().cloned().collect();
    opened.forget(&gone).unwrap();
    opened.prune("test", 10).unwrap();
    let check = opened.check(true).unwrap();
    assert!(check.problems.is_empty(), "{check:?}");
    let last = opened.snapshot(&ids[4]).unwrap();
    let mut out = Vec::new();
    opened
        .read_item(
            &opened.index().unwrap(),
            last.item("kept").unwrap(),
            &mut out,
        )
        .unwrap();
    assert_eq!(out, noise(9, 2_000_000));
    assert!(matches!(
        Repo::open(target.clone(), "wrong"),
        Err(Error::WrongPassword)
    ));
}

/// A TCP proxy to `upstream` that cuts some connections partway: each
/// connection, with a chance of `percent`, is closed after a random number
/// of bytes either way. Returns the address to use instead.
///
/// # Panics
///
/// When it cannot listen on a local port.
pub fn cutting_proxy(
    upstream: std::net::SocketAddr,
    percent: u64,
    seed: u64,
) -> std::net::SocketAddr {
    use std::io::{Read, Write};
    use std::net::{Shutdown, TcpListener, TcpStream};
    use std::sync::atomic::{AtomicU64, Ordering};
    let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
    let address = listener.local_addr().expect("its address");
    let state = std::sync::Arc::new(AtomicU64::new(seed * 2 + 1));
    std::thread::spawn(move || {
        for client in listener.incoming().flatten() {
            let Ok(server) = TcpStream::connect(upstream) else {
                continue;
            };
            // xorshift shared by the connections
            let mut x = state.load(Ordering::SeqCst);
            let mut roll = |n: u64| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x % n.max(1)
            };
            let budget = if roll(100) < percent {
                Some(roll(400_000))
            } else {
                None
            };
            state.store(x, Ordering::SeqCst);
            let pipe = move |mut from: TcpStream, mut to: TcpStream, budget: Option<u64>| {
                let mut buffer = [0u8; 16 * 1024];
                let mut passed = 0u64;
                loop {
                    let n = match from.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    let n = match budget {
                        Some(b) if passed + n as u64 > b => {
                            usize::try_from(b - passed).unwrap_or(0)
                        }
                        _ => n,
                    };
                    if n > 0 && to.write_all(&buffer[..n]).is_err() {
                        break;
                    }
                    passed += n as u64;
                    if budget.is_some_and(|b| passed >= b) {
                        break;
                    }
                }
                let _ = from.shutdown(Shutdown::Both);
                let _ = to.shutdown(Shutdown::Both);
            };
            let (Ok(c2), Ok(s2)) = (client.try_clone(), server.try_clone()) else {
                continue;
            };
            std::thread::spawn(move || pipe(client, server, budget));
            std::thread::spawn(move || pipe(s2, c2, budget));
        }
    });
    address
}

/// Backups and prunes through `cut` (a target whose connections break),
/// then everything checked through `direct` (the same place, whole
/// connections): a full check is clean, every snapshot reads back to the
/// content its items name, the snapshots known saved are there.
///
/// # Panics
///
/// When something does not hold, or nothing ever went through.
pub fn survives_cuts(cut: &Arc<dyn Target>, direct: &Arc<dyn Target>) {
    use sha2::{Digest, Sha256};
    let name = |c: &[u8]| crate::hex(&Sha256::digest(c));
    Repo::init(direct.clone(), "pw", KdfParams::INSECURE_FOR_TESTS, 0).unwrap();
    let mut saved = HashSet::new();
    let mut tries = 0;
    for round in 0..16_u64 {
        let Ok(repo) = Repo::open(cut.clone(), "pw") else {
            continue;
        };
        let now = i64::try_from(round).unwrap() * 31 * 60_000;
        if round % 5 == 4 {
            let _ = repo.prune("test", now);
            continue;
        }
        tries += 1;
        let contents = [
            noise(round % 3, 700_000),
            noise(round + 50, 200_000),
            noise(99, 1_500_000),
        ];
        let run = (|| -> crate::Result<String> {
            let lock = repo.lock("test", false, now)?;
            let index = repo.index()?;
            let mut writer = Writer::new(&repo, &index);
            let mut items = Vec::new();
            for c in &contents {
                let (objects, size) = writer.add_stream(&mut c.as_slice())?;
                items.push(Item {
                    name: name(c),
                    size,
                    objects,
                });
            }
            writer.finish()?;
            let mut snapshot = Snapshot {
                id: String::new(),
                time: now,
                host: "test".into(),
                kind: "test".into(),
                app_version: "0".into(),
                items,
                counts: BTreeMap::new(),
            };
            repo.save_snapshot(&mut snapshot)?;
            let _ = lock.release();
            Ok(snapshot.id)
        })();
        if let Ok(id) = run {
            saved.insert(id);
        }
    }
    let repo = Repo::open(direct.clone(), "pw").unwrap();
    assert!(
        !saved.is_empty(),
        "no backup of {tries} went through the cuts"
    );
    let check = repo.check(true).unwrap();
    assert!(check.problems.is_empty(), "{check:?}");
    let index = repo.index().unwrap();
    let present: HashSet<String> = repo
        .snapshots()
        .unwrap()
        .into_iter()
        .map(|s| s.id)
        .collect();
    for id in &saved {
        assert!(present.contains(id), "snapshot {id} lost");
    }
    for snapshot in repo.snapshots().unwrap() {
        for item in &snapshot.items {
            let mut out = Vec::new();
            repo.read_item(&index, item, &mut out).unwrap();
            assert_eq!(name(&out), item.name);
        }
    }
    repo.prune("test", 100 * 31 * 60_000).unwrap();
    assert!(repo.check(true).unwrap().problems.is_empty());
}
