// SPDX-License-Identifier: AGPL-3.0-or-later

//! Random repositories under stress. Items are named by the hash of their
//! content, so any snapshot can be checked by reading it back, whatever the
//! test knows of it. `DEEP_RUNS` sets the number of seeds (default 4, many more by hand in release).
//!
//! - Two devices back up, thin out and prune at random, on a target that
//!   fails calls at random, sometimes after the write happened (an answer
//!   lost on the way back). After every step: a full check is clean, every
//!   snapshot reads back to exactly its content, none known to be kept went
//!   missing.
//! - Runs and prunes at the same moment, in threads.
//! - Files changed at random: a damaged repository is refused or reads
//!   exactly what it held, never something else.
//! - Retention over two years of irregular runs.

// sizes and counts here are small, made from random numbers
#![expect(
    clippy::cast_possible_truncation,
    reason = "test sizes and counts are small"
)]

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};

use super::target::MemoryTarget;
use super::*;

const KDF: KdfParams = KdfParams::INSECURE_FOR_TESTS;
const MINUTE: i64 = 60_000;

fn runs() -> u64 {
    std::env::var("DEEP_RUNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4)
}

/// xorshift, a failing seed fails the same way every time.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
    fn bytes(&mut self, len: usize) -> Vec<u8> {
        (0..len).map(|_| self.next().to_le_bytes()[0]).collect()
    }
}

/// A target that fails at random: before the call, or after it did its
/// work (the answer lost).
struct Flaky {
    inner: Arc<MemoryTarget>,
    rng: Mutex<Rng>,
    /// Percent of calls that fail.
    rate: Mutex<u64>,
}

impl Flaky {
    fn roll(&self) -> Option<bool> {
        let rate = *self.rate.lock().unwrap();
        let mut rng = self.rng.lock().unwrap();
        // Some(true): fail before, Some(false): fail after the work
        rng.chance(rate).then(|| rng.chance(50))
    }
}

fn cut() -> Error {
    Error::Target("the connection broke".into())
}

impl Target for Flaky {
    fn put(&self, path: &str, bytes: &[u8]) -> Result<()> {
        match self.roll() {
            Some(true) => Err(cut()),
            Some(false) => {
                self.inner.put(path, bytes)?;
                Err(cut())
            }
            None => self.inner.put(path, bytes),
        }
    }
    fn get(&self, path: &str) -> Result<Vec<u8>> {
        if self.roll().is_some() {
            return Err(cut());
        }
        self.inner.get(path)
    }
    fn get_range(&self, path: &str, offset: u64, len: u64) -> Result<Vec<u8>> {
        if self.roll().is_some() {
            return Err(cut());
        }
        self.inner.get_range(path, offset, len)
    }
    fn list(&self, prefix: &str) -> Result<Vec<Listed>> {
        if self.roll().is_some() {
            return Err(cut());
        }
        self.inner.list(prefix)
    }
    fn delete(&self, path: &str) -> Result<()> {
        match self.roll() {
            Some(true) => Err(cut()),
            Some(false) => {
                self.inner.delete(path)?;
                Err(cut())
            }
            None => self.inner.delete(path),
        }
    }
}

fn name_of(content: &[u8]) -> String {
    hex(&Sha256::digest(content))
}

/// A run as a device makes one: its lock, its objects, its snapshot.
fn backup(repo: &Repo, host: &str, now: i64, contents: &[Vec<u8>]) -> Result<String> {
    let lock = repo.lock(host, false, now)?;
    let result = (|| -> Result<String> {
        let index = repo.index()?;
        let mut writer = Writer::new(repo, &index);
        let mut items = Vec::new();
        for content in contents {
            let (objects, size) = writer.add_stream(&mut content.as_slice())?;
            items.push(Item {
                name: name_of(content),
                size,
                objects,
            });
        }
        writer.finish()?;
        let mut snapshot = Snapshot {
            id: String::new(),
            time: now,
            host: host.into(),
            kind: "test".into(),
            app_version: "0".into(),
            items,
            counts: BTreeMap::new(),
        };
        repo.save_snapshot(&mut snapshot)?;
        Ok(snapshot.id)
    })();
    let released = lock.release();
    let id = result?;
    released?;
    Ok(id)
}

/// Every snapshot reads back to the content its items name.
fn verify(repo: &Repo, seed: u64, step: usize) {
    let check = repo.check(true).unwrap();
    assert!(
        check.problems.is_empty(),
        "seed {seed} step {step}: {check:?}"
    );
    let index = repo.index().unwrap();
    for snapshot in repo.snapshots().unwrap() {
        for item in &snapshot.items {
            let mut out = Vec::new();
            repo.read_item(&index, item, &mut out)
                .unwrap_or_else(|e| panic!("seed {seed} step {step}: {}: {e}", item.name));
            assert_eq!(name_of(&out), item.name, "seed {seed} step {step}");
        }
    }
}

#[test]
fn devices_back_up_and_prune_on_a_target_that_fails() {
    for seed in 1..=runs() {
        let mut rng = Rng::new(seed);
        let inner = Arc::new(MemoryTarget::default());
        let flaky = Arc::new(Flaky {
            inner: inner.clone(),
            rng: Mutex::new(Rng::new(seed ^ 0xF1A4)),
            rate: Mutex::new(0),
        });
        let repo = Repo::init(flaky.clone(), "pw", KDF, 0).unwrap();
        // what the checks read: the same files, no faults
        let clean = Repo::open(inner.clone(), "pw").unwrap();
        let mut pool: Vec<Vec<u8>> = (0..12)
            .map(|_| {
                let len = match rng.below(4) {
                    0 => rng.below(200) as usize,
                    1 => 5_000 + rng.below(20_000) as usize,
                    2 => 300_000,
                    _ => 1_300_000 + rng.below(10_000) as usize,
                };
                rng.bytes(len)
            })
            .collect();
        // snapshots known saved and not forgotten: they must stay
        let mut kept: HashSet<String> = HashSet::new();
        for step in 0..30 {
            *flaky.rate.lock().unwrap() = rng.below(25);
            // past any lock a failed release left behind
            let now = i64::try_from(step).unwrap() * 31 * MINUTE;
            let host = if rng.chance(50) { "laptop" } else { "phone" };
            match rng.below(10) {
                0..=5 => {
                    if rng.chance(40) {
                        let at = rng.below(pool.len() as u64) as usize;
                        let mut changed = pool[at].clone();
                        let len = changed.len();
                        let middle = len / 2;
                        changed.splice(middle..middle, rng.bytes(7));
                        pool[at] = changed;
                    }
                    let contents: Vec<Vec<u8>> =
                        pool.iter().filter(|_| rng.chance(60)).cloned().collect();
                    if let Ok(id) = backup(&repo, host, now, &contents) {
                        kept.insert(id);
                    }
                }
                6..=7 => {
                    let mine: Vec<(String, i64)> = clean
                        .snapshots()
                        .unwrap()
                        .into_iter()
                        .filter(|s| s.host == host)
                        .map(|s| (s.id, s.time))
                        .collect();
                    let policy = Policy {
                        last: 1 + rng.below(3) as u32,
                        daily: 0,
                        weekly: 0,
                        monthly: 0,
                        yearly: 0,
                    };
                    let keep = keep(&mine, policy, 0);
                    let gone: HashSet<String> = mine
                        .into_iter()
                        .map(|(id, _)| id)
                        .filter(|id| !keep.contains(id))
                        .collect();
                    // a forget that fails may have removed some: none counted kept
                    for id in &gone {
                        kept.remove(id);
                    }
                    let _ = repo.forget(&gone);
                }
                _ => {
                    let _ = repo.prune(host, now);
                }
            }
            *flaky.rate.lock().unwrap() = 0;
            verify(&clean, seed, step);
            let present: HashSet<String> = clean
                .snapshots()
                .unwrap()
                .into_iter()
                .map(|s| s.id)
                .collect();
            for id in &kept {
                assert!(
                    present.contains(id),
                    "seed {seed} step {step}: snapshot {id} lost"
                );
            }
        }
        // a prune without faults leaves only what is used
        clean.prune("laptop", 10_000 * MINUTE).unwrap();
        verify(&clean, seed, 999);
        let indexed: HashSet<String> = clean.index().unwrap().packs.into_keys().collect();
        for file in inner.list("data/").unwrap() {
            assert!(
                indexed.contains(file.path.rsplit('/').next().unwrap()),
                "seed {seed}: {} left over",
                file.path
            );
        }
    }
}

#[test]
fn runs_and_prunes_at_the_same_moment() {
    let inner = Arc::new(MemoryTarget::default());
    let repo = Arc::new(Repo::init(inner.clone(), "pw", KDF, 0).unwrap());
    let now = || {
        i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap()
    };
    let saved = Arc::new(Mutex::new(HashSet::new()));
    let mut threads = Vec::new();
    for (t, host) in ["laptop", "phone", "desktop"].into_iter().enumerate() {
        let (repo, saved) = (repo.clone(), saved.clone());
        threads.push(std::thread::spawn(move || {
            let mut rng = Rng::new(t as u64 + 100);
            let shared = Rng::new(7).bytes(400_000);
            for _ in 0..25 {
                let contents = vec![shared.clone(), {
                    let len = 50_000 + rng.below(300_000) as usize;
                    rng.bytes(len)
                }];
                if let Ok(id) = backup(&repo, host, now(), &contents) {
                    saved.lock().unwrap().insert(id);
                }
            }
        }));
    }
    let pruner = {
        let (repo, saved) = (repo.clone(), saved.clone());
        std::thread::spawn(move || {
            let mut pruned = 0;
            for _ in 0..40 {
                // forget the oldest half of what exists, then prune
                let mut all: Vec<(String, i64)> = repo
                    .snapshots()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|s| (s.id, s.time))
                    .collect();
                all.sort_by_key(|(_, t)| *t);
                let gone: HashSet<String> = all
                    .iter()
                    .take(all.len() / 2)
                    .map(|(id, _)| id.clone())
                    .collect();
                if repo.forget(&gone).is_ok() {
                    let mut saved = saved.lock().unwrap();
                    for id in &gone {
                        saved.remove(id);
                    }
                }
                if repo.prune("pruner", now()).is_ok() {
                    pruned += 1;
                }
            }
            pruned
        })
    };
    for t in threads {
        t.join().unwrap();
    }
    // how many prunes found the repository free depends on the machine's
    // load: a prune waiting while devices back up is what locks are for
    let _ = pruner.join().unwrap();
    // with the devices done, a prune goes through and removes what is unused
    repo.prune("pruner", now()).unwrap();
    verify(&repo, 0, 0);
    let present: HashSet<String> = repo
        .snapshots()
        .unwrap()
        .into_iter()
        .map(|s| s.id)
        .collect();
    for id in saved.lock().unwrap().iter() {
        assert!(present.contains(id), "snapshot {id} lost");
    }
}

#[test]
fn a_damaged_repository_is_refused_or_reads_what_it_held() {
    for seed in 1..=runs() * 10 {
        let mut rng = Rng::new(seed + 5000);
        let inner = Arc::new(MemoryTarget::default());
        let repo = Repo::init(inner.clone(), "pw", KDF, 0).unwrap();
        let contents: Vec<Vec<u8>> = (0..4).map(|i| rng.bytes(100 + i * 150_000)).collect();
        backup(&repo, "laptop", 1, &contents).unwrap();
        backup(&repo, "laptop", 2, &contents[1..]).unwrap();
        // change bytes of one file, or cut it short, or remove it
        {
            let mut files = inner.files();
            let paths: Vec<String> = files.keys().cloned().collect();
            let path = &paths[rng.below(paths.len() as u64) as usize];
            match rng.below(4) {
                0 => {
                    files.remove(path);
                }
                1 => {
                    let bytes = files.get_mut(path).unwrap();
                    let keep = rng.below(bytes.len() as u64) as usize;
                    bytes.truncate(keep);
                }
                _ => {
                    let bytes = files.get_mut(path).unwrap();
                    if !bytes.is_empty() {
                        for _ in 0..=rng.below(3) {
                            let at = rng.below(bytes.len() as u64) as usize;
                            bytes[at] ^= 1 << rng.below(8);
                        }
                    }
                }
            }
        }
        // opening, listing, checking, reading: never a panic, never
        // something that is not what was stored
        let Ok(opened) = Repo::open(inner.clone(), "pw") else {
            continue;
        };
        let _ = opened.check(true);
        let Ok(index) = opened.index() else { continue };
        let Ok(snapshots) = opened.snapshots() else {
            continue;
        };
        for snapshot in snapshots {
            for item in &snapshot.items {
                let mut out = Vec::new();
                if opened.read_item(&index, item, &mut out).is_ok() {
                    assert_eq!(name_of(&out), item.name, "seed {seed}: read something else");
                }
            }
        }
    }
}

#[test]
fn retention_over_two_years_keeps_what_it_says() {
    const DAY: i64 = 86_400_000;
    for seed in 1..=runs() * 5 {
        let mut rng = Rng::new(seed + 9000);
        // runs at irregular times: some days several, some weeks none
        let mut times = Vec::new();
        let mut t = 1_700_000_000_000_i64;
        while times.len() < 900 {
            t += match rng.below(10) {
                0 => 9 * DAY,
                1..=3 => i64::try_from(rng.below(3_600_000)).unwrap(),
                _ => DAY / 2 + i64::try_from(rng.below(43_200_000)).unwrap(),
            };
            times.push(t);
        }
        let all: Vec<(String, i64)> = times
            .iter()
            .enumerate()
            .map(|(i, t)| (format!("{i}"), *t))
            .collect();
        let policy = Policy {
            last: rng.below(12) as u32,
            daily: rng.below(10) as u32,
            weekly: rng.below(8) as u32,
            monthly: rng.below(14) as u32,
            yearly: rng.below(3) as u32,
        };
        let offset = (i64::try_from(rng.below(24)).unwrap() - 12) * 3600;
        let kept = keep(&all, policy, offset);
        let budget =
            1 + policy.last + policy.daily + policy.weekly + policy.monthly + policy.yearly;
        assert!(
            kept.len() <= budget as usize,
            "seed {seed}: {} kept for {policy:?}",
            kept.len()
        );
        assert!(
            kept.contains(&format!("{}", all.len() - 1)),
            "seed {seed}: the newest"
        );
        // the newest `last` are kept
        for (id, _) in all.iter().rev().take(policy.last as usize) {
            assert!(kept.contains(id), "seed {seed}: one of the last");
        }
        // the newest of each of the latest `daily` days with runs is kept
        let day = |t: i64| (t / 1000 + offset).div_euclid(86_400);
        let mut days: Vec<i64> = all.iter().map(|(_, t)| day(*t)).collect();
        days.dedup();
        for d in days.iter().rev().take(policy.daily as usize) {
            let newest = all.iter().rev().find(|(_, t)| day(*t) == *d).unwrap();
            assert!(kept.contains(&newest.0), "seed {seed}: day {d}");
        }
        // keeping again what is kept changes nothing
        let again: Vec<(String, i64)> = all
            .iter()
            .filter(|(id, _)| kept.contains(id))
            .cloned()
            .collect();
        assert_eq!(keep(&again, policy, offset), kept, "seed {seed}: stable");
    }
}
