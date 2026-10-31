// SPDX-License-Identifier: AGPL-3.0-or-later

//! Writing a run: objects the repository does not hold yet go into packs
//! of up to 16 MB, each pack is written whole, then one index file for the
//! run. Objects it holds are only counted.

use std::collections::HashSet;
use std::io::Read;

use sha2::{Digest, Sha256};

use super::{Index, IndexFile, PackEntry, Repo, chunk, pack_path, pack_plain, random_hex};
use crate::error::Result;

/// A pack is written once it holds this much.
pub const PACK_SIZE: usize = 16 * 1024 * 1024;

/// What a run wrote and what it found there already.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WriteStats {
    /// Objects written.
    pub new_objects: u64,
    /// Their plain size.
    pub new_bytes: u64,
    /// What went to the target, sealed and compressed.
    pub stored_bytes: u64,
    /// Objects the repository had already.
    pub known_objects: u64,
    /// Plain bytes read in all.
    pub read_bytes: u64,
    /// Packs written.
    pub packs: u64,
}

/// Adds objects to a repository.
pub struct Writer<'a> {
    repo: &'a Repo,
    known: HashSet<String>,
    pack: Vec<u8>,
    pack_objects: Vec<(String, u64, u64)>,
    written: Vec<PackEntry>,
    stats: WriteStats,
    /// Called after each pack, to keep a lock fresh.
    on_pack: Option<Box<dyn FnMut() -> Result<()> + 'a>>,
}

impl<'a> Writer<'a> {
    /// A writer that skips what `index` holds.
    pub fn new(repo: &'a Repo, index: &Index) -> Self {
        Self {
            repo,
            known: index.objects.keys().cloned().collect(),
            pack: Vec::new(),
            pack_objects: Vec::new(),
            written: Vec::new(),
            stats: WriteStats::default(),
            on_pack: None,
        }
    }

    /// Runs `f` after each pack written.
    pub fn on_pack(&mut self, f: impl FnMut() -> Result<()> + 'a) {
        self.on_pack = Some(Box::new(f));
    }

    /// Whether the repository (or this run) holds the object.
    pub fn has(&self, id: &str) -> bool {
        self.known.contains(id)
    }

    /// Adds content as one object, returns its id.
    ///
    /// # Errors
    ///
    /// Crypto and target errors.
    pub fn add(&mut self, plain: &[u8]) -> Result<String> {
        let id = self.repo.object_id(plain);
        self.stats.read_bytes += plain.len() as u64;
        if self.known.contains(&id) {
            self.stats.known_objects += 1;
            return Ok(id);
        }
        let sealed = self.repo.seal(b"object", &pack_plain(plain)?)?;
        let offset = self.pack.len() as u64;
        self.pack.extend_from_slice(&sealed);
        self.pack_objects
            .push((id.clone(), offset, sealed.len() as u64));
        self.known.insert(id.clone());
        self.stats.new_objects += 1;
        self.stats.new_bytes += plain.len() as u64;
        self.stats.stored_bytes += sealed.len() as u64;
        if self.pack.len() >= PACK_SIZE {
            self.flush()?;
        }
        Ok(id)
    }

    /// Adds content of any size, cut into chunks: their ids and the size.
    ///
    /// # Errors
    ///
    /// Reading, crypto and target errors.
    pub fn add_stream(&mut self, source: &mut dyn Read) -> Result<(Vec<String>, u64)> {
        let mut ids = Vec::new();
        let mut size = 0u64;
        chunk::chunks(source, |piece| {
            size += piece.len() as u64;
            ids.push(self.add(piece)?);
            Ok(())
        })?;
        Ok((ids, size))
    }

    fn flush(&mut self) -> Result<()> {
        if self.pack.is_empty() {
            return Ok(());
        }
        let name = crate::hex(&Sha256::digest(&self.pack));
        self.repo.target.put(&pack_path(&name), &self.pack)?;
        self.written.push(PackEntry {
            name,
            size: self.pack.len() as u64,
            objects: std::mem::take(&mut self.pack_objects),
        });
        self.pack.clear();
        self.stats.packs += 1;
        if let Some(f) = self.on_pack.as_mut() {
            f()?;
        }
        Ok(())
    }

    /// Writes the last pack and the run's index file.
    ///
    /// # Errors
    ///
    /// Crypto and target errors.
    pub fn finish(mut self) -> Result<WriteStats> {
        self.flush()?;
        if !self.written.is_empty() {
            let file = IndexFile {
                packs: std::mem::take(&mut self.written),
            };
            self.repo
                .write_json(b"index", &format!("index/{}", random_hex()?), &file)?;
        }
        Ok(self.stats)
    }
}
