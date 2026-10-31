// SPDX-License-Identifier: AGPL-3.0-or-later

//! Keeping a repository in order: locks so two devices do not prune under
//! each other, prune to remove what no snapshot uses, check to find damage.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{Index, Repo, Writer, pack_path, random_hex, unpack_plain};
use crate::error::{Error, Result};

/// A lock older than this is from a run that died: it no longer counts.
pub const STALE_MS: i64 = 30 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LockFile {
    host: String,
    time: i64,
    exclusive: bool,
}

/// A lock held on a repository: shared for a backup (several may run),
/// exclusive for a prune (alone).
pub struct Lock<'a> {
    repo: &'a Repo,
    path: String,
    file: LockFile,
}

impl std::fmt::Debug for Lock<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lock")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl Repo {
    /// The fresh locks other than `own`.
    fn others(&self, own: &str, now: i64) -> Result<Vec<LockFile>> {
        let mut out = Vec::new();
        for listed in self.target.list("locks/")? {
            if listed.path == own {
                continue;
            }
            // a lock that does not open is not ours to honour
            if let Ok(lock) = self.read_json::<LockFile>(b"lock", &listed.path)
                && now - lock.time < STALE_MS
            {
                out.push(lock);
            }
        }
        Ok(out)
    }

    /// Takes a lock, after checking nobody holds one in the way. Checked
    /// again once written, as two may take one at the same moment.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] naming who uses the repository, target errors.
    pub fn lock(&self, host: &str, exclusive: bool, now: i64) -> Result<Lock<'_>> {
        let in_the_way = |locks: &[LockFile]| {
            locks
                .iter()
                .find(|l| exclusive || l.exclusive)
                .map(|l| Error::Invalid(format!("the backup is in use by {}", l.host)))
        };
        if let Some(e) = in_the_way(&self.others("", now)?) {
            return Err(e);
        }
        let lock = Lock {
            repo: self,
            path: format!("locks/{}", random_hex()?),
            file: LockFile {
                host: host.to_owned(),
                time: now,
                exclusive,
            },
        };
        self.write_json(b"lock", &lock.path, &lock.file)?;
        if let Some(e) = in_the_way(&self.others(&lock.path, now)?) {
            let _ = self.target.delete(&lock.path);
            return Err(e);
        }
        Ok(lock)
    }
}

impl Lock<'_> {
    /// Keeps the lock from going stale during a long run.
    ///
    /// # Errors
    ///
    /// Target errors.
    pub fn refresh(&mut self, now: i64) -> Result<()> {
        if now - self.file.time > STALE_MS / 3 {
            self.file.time = now;
            self.repo.write_json(b"lock", &self.path, &self.file)?;
        }
        Ok(())
    }

    /// Gives the lock up.
    ///
    /// # Errors
    ///
    /// Target errors (the lock then goes stale on its own).
    pub fn release(self) -> Result<()> {
        self.repo.target.delete(&self.path)
    }
}

/// What a prune did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PruneReport {
    /// Packs removed.
    pub packs_removed: u64,
    /// Packs rewritten with only what is used.
    pub packs_rewritten: u64,
    /// Objects no snapshot used, gone.
    pub objects_removed: u64,
    /// Bytes freed at the target.
    pub bytes_freed: u64,
}

/// A pack is rewritten when more than this many tenths of its objects
/// are unused.
const REWRITE_UNUSED_TENTHS: usize = 3;

impl Repo {
    /// Removes what no snapshot uses: packs with nothing used go, packs
    /// mostly unused are rewritten. New packs and the new index are
    /// written before anything is removed, so a prune cut short loses
    /// nothing. Takes the exclusive lock.
    ///
    /// # Errors
    ///
    /// The repository is in use, target errors.
    pub fn prune(&self, host: &str, now: i64) -> Result<PruneReport> {
        let lock = self.lock(host, true, now)?;
        let result = self.prune_locked();
        lock.release()?;
        result
    }

    /// The objects the snapshots use and the index, read so that a
    /// listing cut short but taken as whole cannot make used data look
    /// unused: everything used must be indexed, and a second look must see
    /// the same files, before a prune removes anything.
    fn whole_view(&self) -> Result<(HashSet<String>, Index)> {
        let names = |prefix: &str| -> Result<HashSet<String>> {
            Ok(self
                .target
                .list(prefix)?
                .into_iter()
                .map(|l| l.path)
                .collect())
        };
        let (snapshot_files, index_files) = (names("snapshots/")?, names("index/")?);
        let used: HashSet<String> = self
            .snapshots()?
            .into_iter()
            .flat_map(|s| s.items.into_iter().flat_map(|i| i.objects))
            .collect();
        let index = self.index()?;
        if used.iter().any(|id| !index.objects.contains_key(id)) {
            return Err(Error::Corrupt(
                "a snapshot uses an object no index has: check the backup before pruning".into(),
            ));
        }
        if names("snapshots/")? != snapshot_files || names("index/")? != index_files {
            return Err(Error::Target(
                "the backup's listing changed while pruning: try again".into(),
            ));
        }
        Ok((used, index))
    }

    fn prune_locked(&self) -> Result<PruneReport> {
        let mut report = PruneReport::default();
        let (used, index) = self.whole_view()?;
        // what each pack holds that is used, and what it holds in all
        let mut by_pack: HashMap<&str, (Vec<&str>, usize, u64)> = HashMap::new();
        for (id, at) in &index.objects {
            let entry = by_pack.entry(&at.pack).or_default();
            entry.1 += 1;
            if used.contains(id) {
                entry.0.push(id);
            } else {
                entry.2 += at.length;
            }
        }
        let mut drop_packs: Vec<String> = Vec::new();
        let mut rewrite: Vec<&str> = Vec::new();
        for (pack, (kept, total, unused_bytes)) in &by_pack {
            if kept.is_empty() {
                drop_packs.push((*pack).to_owned());
            } else if (*total - kept.len()) * 10 > *total * REWRITE_UNUSED_TENTHS {
                rewrite.push(pack);
                report.bytes_freed += unused_bytes;
            }
        }
        // the new index: what stays where it is, and what moves
        let mut fresh = Index::default();
        let rewritten: HashSet<&str> = rewrite.iter().copied().collect();
        let dropped: HashSet<&str> = drop_packs.iter().map(String::as_str).collect();
        for (id, at) in &index.objects {
            if used.contains(id) && !rewritten.contains(at.pack.as_str()) {
                fresh.objects.insert(id.clone(), at.clone());
            }
        }
        let mut writer = Writer::new(self, &fresh);
        for pack in &rewrite {
            for id in &by_pack[pack].0 {
                let plain = self.read_object(&index, id)?;
                writer.add(&plain)?;
            }
        }
        writer.finish()?;
        // the objects that stay where they were, in one more index file:
        // with it and the rewrite's index, every used object is listed
        let mut keep_file = super::IndexFile::default();
        for (pack, (size, _)) in &index.packs {
            if rewritten.contains(pack.as_str()) || dropped.contains(pack.as_str()) {
                continue;
            }
            let objects: Vec<(String, u64, u64)> = fresh
                .objects
                .iter()
                .filter(|(_, at)| &at.pack == pack)
                .map(|(id, at)| (id.clone(), at.offset, at.length))
                .collect();
            keep_file.packs.push(super::PackEntry {
                name: pack.clone(),
                size: *size,
                objects,
            });
        }
        let keep_path = format!("index/{}", random_hex()?);
        self.write_json(b"index", &keep_path, &keep_file)?;
        // everything needed is listed by the new files: the old ones go
        let new_files: HashSet<String> = self
            .target
            .list("index/")?
            .into_iter()
            .map(|l| l.path)
            .filter(|p| !index.files.contains(p))
            .collect();
        for file in &index.files {
            if !new_files.contains(file) {
                self.target.delete(file)?;
            }
        }
        for pack in &drop_packs {
            report.bytes_freed += index.packs.get(pack).map_or(0, |(size, _)| *size);
            self.target.delete(&pack_path(pack))?;
        }
        for pack in &rewrite {
            self.target.delete(&pack_path(pack))?;
        }
        // packs in no index: left by runs cut short before their index
        let listed: HashSet<String> = index.packs.keys().cloned().collect();
        let after = self.index()?;
        for file in self.target.list("data/")? {
            let name = file.path.rsplit('/').next().unwrap_or_default();
            if !listed.contains(name) && !after.packs.contains_key(name) {
                report.bytes_freed += file.size;
                report.packs_removed += 1;
                self.target.delete(&file.path)?;
            }
        }
        report.packs_removed += drop_packs.len() as u64;
        report.packs_rewritten = rewrite.len() as u64;
        report.objects_removed = index
            .objects
            .keys()
            .filter(|id| !used.contains(*id))
            .count() as u64;
        Ok(report)
    }
}

/// What a check found.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckReport {
    /// Snapshots read.
    pub snapshots: u64,
    /// Packs looked at.
    pub packs: u64,
    /// Objects the snapshots use.
    pub objects: u64,
    /// Bytes at the target.
    pub stored_bytes: u64,
    /// What is wrong, in words. Empty when all is well.
    pub problems: Vec<String>,
}

impl Repo {
    /// Checks the repository: every snapshot opens, every object it uses is
    /// in an index, every pack an index names is there with its size. With
    /// `read_data`, every pack is read whole: its hash, then each object's
    /// seal and id.
    ///
    /// # Errors
    ///
    /// Target errors. Damage is reported, not returned.
    pub fn check(&self, read_data: bool) -> Result<CheckReport> {
        let mut report = CheckReport::default();
        let index = match self.index() {
            Ok(index) => index,
            Err(Error::Corrupt(why)) => {
                report.problems.push(format!("an index: {why}"));
                return Ok(report);
            }
            Err(e) => return Err(e),
        };
        let mut used = HashSet::new();
        for listed in self.target.list("snapshots/")? {
            let id = listed
                .path
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_owned();
            match self.snapshot(&id) {
                Ok(snapshot) => {
                    report.snapshots += 1;
                    for item in &snapshot.items {
                        for object in &item.objects {
                            if !index.objects.contains_key(object) {
                                report.problems.push(format!(
                                    "snapshot {id}: {} needs an object no index has",
                                    item.name
                                ));
                            }
                            used.insert(object.clone());
                        }
                    }
                }
                Err(Error::Corrupt(why)) => report.problems.push(format!("snapshot {id}: {why}")),
                Err(e) => return Err(e),
            }
        }
        report.objects = used.len() as u64;
        let present: HashMap<String, u64> = self
            .target
            .list("data/")?
            .into_iter()
            .map(|l| {
                (
                    l.path.rsplit('/').next().unwrap_or_default().to_owned(),
                    l.size,
                )
            })
            .collect();
        for (pack, (size, _)) in &index.packs {
            report.packs += 1;
            match present.get(pack) {
                None => report.problems.push(format!("pack {pack} is missing")),
                Some(have) if have != size => {
                    report
                        .problems
                        .push(format!("pack {pack} is {have} bytes, not {size}"));
                }
                Some(_) => report.stored_bytes += size,
            }
        }
        if read_data {
            let mut by_pack: HashMap<&str, Vec<(&str, u64, u64)>> = HashMap::new();
            for (id, at) in &index.objects {
                if used.contains(id) {
                    by_pack
                        .entry(&at.pack)
                        .or_default()
                        .push((id, at.offset, at.length));
                }
            }
            for (pack, objects) in by_pack {
                if !present.contains_key(pack) {
                    continue;
                }
                let bytes = self.target.get(&pack_path(pack))?;
                if crate::hex(&Sha256::digest(&bytes)) != pack {
                    report.problems.push(format!("pack {pack} is damaged"));
                    continue;
                }
                for (id, offset, length) in objects {
                    let opened = usize::try_from(offset)
                        .ok()
                        .zip(usize::try_from(offset + length).ok())
                        .and_then(|(a, b)| bytes.get(a..b))
                        .ok_or_else(|| Error::Corrupt("outside its pack".into()))
                        .and_then(|sealed| self.unseal(b"object", sealed))
                        .and_then(|plain| unpack_plain(&plain));
                    match opened {
                        Ok(plain) if self.object_id(&plain) == id => {}
                        Ok(_) => report
                            .problems
                            .push(format!("object {id} is not what it names")),
                        Err(e) => report.problems.push(format!("object {id}: {e}")),
                    }
                }
            }
        }
        report.problems.sort();
        Ok(report)
    }
}
