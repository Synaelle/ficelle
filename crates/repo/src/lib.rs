// SPDX-License-Identifier: AGPL-3.0-or-later

//! Backup repositories: encrypted, incremental, deduplicated snapshots kept
//! on a [`Target`] (a folder, S3, `WebDAV`...). Not the backup archive of
//! the core, one file made by hand: a repository is filled by scheduled
//! runs and keeps many snapshots, each stored once whatever it shares with
//! the others. What goes in a snapshot is the caller's: an app's notes
//! (the core), a server's database and files (the server).
//!
//! Layout, format 1:
//!
//! - `config`: the format and the repository's keys, sealed under keys
//!   derived from its passwords (Argon2id).
//! - `data/<2 hex>/<sha256>`: packs, sealed objects one after the other.
//!   A pack is named by the hash of its bytes, so it can be checked
//!   without its keys.
//! - `index/<id>`: where each object is, which pack, offset and length.
//! - `snapshots/<id>`: what a run backed up, by object ids.
//! - `locks/<id>`: who is using the repository.
//!
//! An object's id is HMAC-SHA256 of its plain bytes under the repository's
//! id key: the same content is stored once, and ids tell nothing about it.
//! Everything but `config` is sealed with XChaCha20-Poly1305 under the data
//! key, bound to what it is (an object cannot pass for a snapshot).

pub mod chunk;
mod error;
mod http;
mod maintain;
mod s3;
mod snapshot;
pub mod target;
#[cfg(feature = "testing")]
pub mod testing;
mod webdav;
mod writer;

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

pub use error::{Error, Result};
pub use maintain::{CheckReport, Lock, PruneReport};
pub use s3::{S3Config, S3Target};
pub use snapshot::{Item, Policy, Snapshot, keep};
pub use target::{FolderTarget, Listed, Target};
pub use webdav::{WebdavConfig, WebdavTarget};
pub use writer::{WriteStats, Writer};

/// The format this version writes, and the newest it reads.
pub const FORMAT: u32 = 1;

const NONCE: usize = 24;
const SALT: usize = 16;

/// Argon2id settings a repository may ask for, at most: a hostile config
/// cannot make opening it take the device's memory.
const MAX_KDF: KdfParams = KdfParams {
    memory_kib: 1024 * 1024,
    iterations: 32,
    parallelism: 16,
};

/// A password's way to the repository's keys.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KeySlot {
    kdf: KdfParams,
    salt: String,
    /// The data key then the id key, sealed under the password's key.
    sealed: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
    format: u32,
    created_at: i64,
    keys: Vec<KeySlot>,
}

struct Keys {
    data: Zeroizing<[u8; 32]>,
    id: Zeroizing<[u8; 32]>,
}

/// Where an object is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    /// The pack's name.
    pub pack: String,
    /// Its sealed bytes start here in the pack.
    pub offset: u64,
    /// And are this long.
    pub length: u64,
}

/// One index file: the packs a run wrote and their objects.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IndexFile {
    packs: Vec<PackEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackEntry {
    name: String,
    size: u64,
    /// Object id, offset, length.
    objects: Vec<(String, u64, u64)>,
}

/// Every index file of the repository, read together.
#[derive(Debug, Default)]
pub struct Index {
    /// Where each object is.
    pub objects: HashMap<String, Location>,
    /// Pack name to its size and how many objects it holds.
    pub packs: HashMap<String, (u64, usize)>,
    /// The index files read.
    pub files: Vec<String>,
}

/// An open repository.
pub struct Repo {
    target: Arc<dyn Target>,
    keys: Keys,
}

impl std::fmt::Debug for Repo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Repo").finish_non_exhaustive()
    }
}

/// Argon2id settings for a password's key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KdfParams {
    /// Memory in KiB.
    pub memory_kib: u32,
    /// Passes over memory.
    pub iterations: u32,
    /// Lanes.
    pub parallelism: u32,
}

impl KdfParams {
    /// What a repository is made with outside tests: as the vaults.
    pub const DEFAULT: Self = Self {
        memory_kib: 64 * 1024,
        iterations: 3,
        parallelism: 1,
    };

    /// Fast and weak, for tests only.
    pub const INSECURE_FOR_TESTS: Self = Self {
        memory_kib: 8,
        iterations: 1,
        parallelism: 1,
    };

    fn derive_key(self, password: &str, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>> {
        use argon2::{Algorithm, Argon2, Params, Version};
        let params = Params::new(self.memory_kib, self.iterations, self.parallelism, Some(32))
            .map_err(|_| Error::Invalid("unsupported key derivation settings".into()))?;
        let mut out = Zeroizing::new([0u8; 32]);
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password_into(password.as_bytes(), salt, out.as_mut())
            .map_err(|_| Error::Crypto)?;
        Ok(out)
    }
}

fn random_bytes<const N: usize>() -> Result<[u8; N]> {
    let mut out = [0u8; N];
    getrandom::fill(&mut out).map_err(|_| Error::Crypto)?;
    Ok(out)
}

/// Lower case hex.
pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
            // writing to a String cannot fail
            let _ = write!(out, "{b:02x}");
            out
        })
}

fn random_hex() -> Result<String> {
    Ok(hex(&random_bytes::<16>()?))
}

fn cipher(key: &[u8; 32]) -> Result<XChaCha20Poly1305> {
    XChaCha20Poly1305::new_from_slice(key).map_err(|_| Error::Crypto)
}

fn slot(data: &[u8; 32], id: &[u8; 32], password: &str, kdf: KdfParams) -> Result<KeySlot> {
    let salt: [u8; SALT] = random_bytes()?;
    let nonce: [u8; NONCE] = random_bytes()?;
    let key = kdf.derive_key(password, &salt)?;
    let mut both = Zeroizing::new([0u8; 64]);
    both[..32].copy_from_slice(data);
    both[32..].copy_from_slice(id);
    let sealed = cipher(&key)?
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: both.as_ref(),
                aad: b"repository keys",
            },
        )
        .map_err(|_| Error::Crypto)?;
    Ok(KeySlot {
        kdf,
        salt: BASE64.encode(salt),
        sealed: BASE64.encode([nonce.as_slice(), &sealed].concat()),
    })
}

fn unseal_slot(slot: &KeySlot, password: &str) -> Result<Option<Keys>> {
    let kdf = slot.kdf;
    if kdf.memory_kib > MAX_KDF.memory_kib
        || kdf.iterations > MAX_KDF.iterations
        || kdf.parallelism > MAX_KDF.parallelism
    {
        return Err(Error::Corrupt(
            "repository key settings out of range".into(),
        ));
    }
    let decode = |text: &str| {
        BASE64
            .decode(text)
            .map_err(|_| Error::Corrupt("repository config".into()))
    };
    let salt = decode(&slot.salt)?;
    let sealed = decode(&slot.sealed)?;
    if sealed.len() < NONCE {
        return Err(Error::Corrupt("repository config".into()));
    }
    let key = kdf.derive_key(password, &salt)?;
    let (nonce, body) = sealed.split_at(NONCE);
    let nonce: [u8; NONCE] = nonce.try_into().map_err(|_| Error::Crypto)?;
    let Ok(plain) = cipher(&key)?.decrypt(
        &XNonce::from(nonce),
        Payload {
            msg: body,
            aad: b"repository keys",
        },
    ) else {
        return Ok(None);
    };
    let plain = Zeroizing::new(plain);
    if plain.len() != 64 {
        return Err(Error::Corrupt("repository keys".into()));
    }
    let mut data = Zeroizing::new([0u8; 32]);
    let mut id = Zeroizing::new([0u8; 32]);
    data.copy_from_slice(&plain[..32]);
    id.copy_from_slice(&plain[32..]);
    Ok(Some(Keys { data, id }))
}

fn pack_path(name: &str) -> String {
    format!("data/{}/{name}", &name[..2.min(name.len())])
}

/// Content compressed when that makes it smaller, with a byte saying so.
fn pack_plain(plain: &[u8]) -> Result<Vec<u8>> {
    let mut deflate = flate2::write::DeflateEncoder::new(vec![1u8], flate2::Compression::fast());
    deflate.write_all(plain)?;
    let compressed = deflate.finish()?;
    if compressed.len() < plain.len() + 1 {
        Ok(compressed)
    } else {
        Ok([&[0u8][..], plain].concat())
    }
}

fn unpack_plain(stored: &[u8]) -> Result<Vec<u8>> {
    match stored.split_first() {
        Some((0, rest)) => Ok(rest.to_vec()),
        Some((1, rest)) => {
            let mut out = Vec::new();
            // objects are at most a chunk, a bomb stops there
            flate2::read::DeflateDecoder::new(rest)
                .take(chunk::MAX as u64 + 1)
                .read_to_end(&mut out)?;
            if out.len() > chunk::MAX {
                return Err(Error::Corrupt("an object larger than any chunk".into()));
            }
            Ok(out)
        }
        _ => Err(Error::Corrupt("an object of unknown kind".into())),
    }
}

impl Repo {
    /// Makes a repository at `target`, opened with `password`.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] when one is there already, target errors.
    pub fn init(target: Arc<dyn Target>, password: &str, kdf: KdfParams, now: i64) -> Result<Self> {
        if password.is_empty() {
            return Err(Error::Invalid("a backup needs a password".into()));
        }
        match target.get("config") {
            Ok(_) => {
                return Err(Error::Invalid(
                    "there is a backup repository there already".into(),
                ));
            }
            Err(Error::NotFound(_)) => {}
            Err(e) => return Err(e),
        }
        let keys = Keys {
            data: Zeroizing::new(random_bytes()?),
            id: Zeroizing::new(random_bytes()?),
        };
        let config = Config {
            format: FORMAT,
            created_at: now,
            keys: vec![slot(&keys.data, &keys.id, password, kdf)?],
        };
        let text = serde_json::to_vec_pretty(&config).map_err(|e| Error::Invalid(e.to_string()))?;
        target.put("config", &text)?;
        Ok(Self { target, keys })
    }

    /// Opens the repository at `target` with its password.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`] when there is none, [`Error::WrongPassword`],
    /// [`Error::Corrupt`] for a damaged config or a newer format.
    pub fn open(target: Arc<dyn Target>, password: &str) -> Result<Self> {
        let config: Config =
            serde_json::from_slice(&target.get("config").map_err(|e| match e {
                Error::NotFound(_) => Error::NotFound("backup repository"),
                other => other,
            })?)
            .map_err(|e| Error::Corrupt(format!("repository config: {e}")))?;
        if config.format > FORMAT {
            return Err(Error::Corrupt(
                "this repository was made by a newer version".into(),
            ));
        }
        for slot in config.keys.iter().take(16) {
            if let Some(keys) = unseal_slot(slot, password)? {
                return Ok(Self { target, keys });
            }
        }
        Err(Error::WrongPassword)
    }

    /// Whether `target` holds a repository.
    ///
    /// # Errors
    ///
    /// Target errors.
    pub fn exists(target: &dyn Target) -> Result<bool> {
        match target.get("config") {
            Ok(_) => Ok(true),
            Err(Error::NotFound(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Replaces the repository's password: the keys stay, so nothing is
    /// sealed again.
    ///
    /// # Errors
    ///
    /// Target errors.
    pub fn change_password(&self, password: &str, kdf: KdfParams, now: i64) -> Result<()> {
        if password.is_empty() {
            return Err(Error::Invalid("a backup needs a password".into()));
        }
        let created_at = serde_json::from_slice::<Config>(&self.target.get("config")?)
            .map_or(now, |c| c.created_at);
        let config = Config {
            format: FORMAT,
            created_at,
            keys: vec![slot(&self.keys.data, &self.keys.id, password, kdf)?],
        };
        let text = serde_json::to_vec_pretty(&config).map_err(|e| Error::Invalid(e.to_string()))?;
        self.target.put("config", &text)
    }

    /// Where the repository is.
    pub fn target(&self) -> &dyn Target {
        self.target.as_ref()
    }

    /// The id of plain content.
    ///
    /// # Panics
    ///
    /// Never: HMAC takes a key of any length.
    pub fn object_id(&self, plain: &[u8]) -> String {
        use hmac::{Hmac, KeyInit as _, Mac};
        let mut mac = <Hmac<sha2::Sha256>>::new_from_slice(self.keys.id.as_ref())
            .expect("HMAC takes any key length");
        mac.update(plain);
        hex(&mac.finalize().into_bytes())
    }

    fn seal(&self, kind: &[u8], plain: &[u8]) -> Result<Vec<u8>> {
        let nonce: [u8; NONCE] = random_bytes()?;
        let sealed = cipher(&self.keys.data)?
            .encrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: plain,
                    aad: kind,
                },
            )
            .map_err(|_| Error::Crypto)?;
        Ok([nonce.as_slice(), &sealed].concat())
    }

    fn unseal(&self, kind: &[u8], sealed: &[u8]) -> Result<Vec<u8>> {
        if sealed.len() < NONCE {
            return Err(Error::Corrupt("a repository file is cut short".into()));
        }
        let (nonce, body) = sealed.split_at(NONCE);
        let nonce: [u8; NONCE] = nonce.try_into().map_err(|_| Error::Crypto)?;
        cipher(&self.keys.data)?
            .decrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: body,
                    aad: kind,
                },
            )
            .map_err(|_| {
                Error::Corrupt(
                    "a repository file does not open: damaged or not of this repository".into(),
                )
            })
    }

    fn read_json<T: serde::de::DeserializeOwned>(&self, kind: &[u8], path: &str) -> Result<T> {
        let plain = self.unseal(kind, &self.target.get(path)?)?;
        serde_json::from_slice(&plain).map_err(|e| Error::Corrupt(format!("{path}: {e}")))
    }

    fn write_json<T: Serialize>(&self, kind: &[u8], path: &str, value: &T) -> Result<()> {
        let plain = serde_json::to_vec(value).map_err(|e| Error::Invalid(e.to_string()))?;
        self.target.put(path, &self.seal(kind, &plain)?)
    }

    /// Reads every index file.
    ///
    /// # Errors
    ///
    /// Target errors, a damaged index.
    pub fn index(&self) -> Result<Index> {
        let mut index = Index::default();
        for listed in self.target.list("index/")? {
            let file: IndexFile = self.read_json(b"index", &listed.path)?;
            for pack in file.packs {
                for (id, offset, length) in &pack.objects {
                    index.objects.entry(id.clone()).or_insert_with(|| Location {
                        pack: pack.name.clone(),
                        offset: *offset,
                        length: *length,
                    });
                }
                index
                    .packs
                    .insert(pack.name.clone(), (pack.size, pack.objects.len()));
            }
            index.files.push(listed.path);
        }
        Ok(index)
    }

    /// The plain content of an object, checked against its id.
    ///
    /// # Errors
    ///
    /// [`Error::Corrupt`] when it is not in the index, does not open or is
    /// not the content its id names.
    pub fn read_object(&self, index: &Index, id: &str) -> Result<Vec<u8>> {
        let at = index
            .objects
            .get(id)
            .ok_or_else(|| Error::Corrupt(format!("object {id} is in no index")))?;
        let sealed = self
            .target
            .get_range(&pack_path(&at.pack), at.offset, at.length)?;
        let plain = unpack_plain(&self.unseal(b"object", &sealed)?)?;
        if self.object_id(&plain) != id {
            return Err(Error::Corrupt(format!("object {id} is not what it names")));
        }
        Ok(plain)
    }

    /// Writes an item's content, its objects one after the other.
    ///
    /// # Errors
    ///
    /// As [`Repo::read_object`], and the writer's errors.
    pub fn read_item(&self, index: &Index, item: &Item, out: &mut dyn Write) -> Result<()> {
        let mut size = 0u64;
        for id in &item.objects {
            let plain = self.read_object(index, id)?;
            size += plain.len() as u64;
            out.write_all(&plain)?;
        }
        if size != item.size {
            return Err(Error::Corrupt(format!(
                "{} is not the size it was",
                item.name
            )));
        }
        Ok(())
    }

    /// The snapshots, oldest first.
    ///
    /// # Errors
    ///
    /// Target errors, a damaged snapshot.
    pub fn snapshots(&self) -> Result<Vec<Snapshot>> {
        let mut out = Vec::new();
        for listed in self.target.list("snapshots/")? {
            let mut snapshot: Snapshot = self.read_json(b"snapshot", &listed.path)?;
            listed
                .path
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .clone_into(&mut snapshot.id);
            out.push(snapshot);
        }
        out.sort_by(|a, b| a.time.cmp(&b.time).then_with(|| a.id.cmp(&b.id)));
        Ok(out)
    }

    /// One snapshot by its id.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`] when there is none.
    pub fn snapshot(&self, id: &str) -> Result<Snapshot> {
        if !target::safe_path(id) || id.contains('/') {
            return Err(Error::NotFound("snapshot"));
        }
        let mut snapshot: Snapshot = self
            .read_json(b"snapshot", &format!("snapshots/{id}"))
            .map_err(|e| match e {
                Error::NotFound(_) => Error::NotFound("snapshot"),
                other => other,
            })?;
        id.clone_into(&mut snapshot.id);
        Ok(snapshot)
    }

    /// Writes a snapshot under a new id, set in it.
    ///
    /// # Errors
    ///
    /// Crypto and target errors.
    pub fn save_snapshot(&self, snapshot: &mut Snapshot) -> Result<()> {
        snapshot.id = random_hex()?;
        self.write_json(b"snapshot", &format!("snapshots/{}", snapshot.id), snapshot)
    }

    /// Removes snapshots by id. Their objects stay until a prune.
    ///
    /// # Errors
    ///
    /// Target errors.
    pub fn forget(&self, ids: &HashSet<String>) -> Result<()> {
        for id in ids {
            if target::safe_path(id) && !id.contains('/') {
                self.target.delete(&format!("snapshots/{id}"))?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod deep;
#[cfg(test)]
mod tests;
