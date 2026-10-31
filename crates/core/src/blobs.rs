// SPDX-License-Identifier: AGPL-3.0-or-later

//! Attachment files, stored once per content (deduplicated by SHA-256).
//!
//! Plain vaults keep files as they are, named by their hash. Encrypted vaults
//! seal each file with a key kept in the encrypted database, in the stream
//! format of [`app_e2ee::seal_file`] (64 KB chunks, so a file of any size is
//! written and read a piece at a time), and name it by an HMAC of the hash,
//! so the folder does not reveal which known files are present. The hash is
//! bound to every chunk, so files cannot be swapped unnoticed.
//!
//! Files written before phase 8 were sealed whole (format 1): they are still
//! read, whole, never written.

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

use app_e2ee::{CHUNK, FILE_FORMAT, FILE_HEADER, FilesKey, Opener, SEALED_CHUNK, Sealer};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::crypto::random_bytes;
use crate::error::{Error, Result};
use crate::export::AttachmentSource;

/// First byte of a file sealed whole, before phase 8.
const FORMAT_V1: u8 = 1;
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;

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

/// A file's hash as the store names it: 64 lowercase hex characters. Hashes
/// come from synced changes and backups too, so nothing else may become a
/// path (an absolute path in its place would
/// replace the store's folder).
pub fn is_sha256_hex(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Lowercase hex SHA-256 of some bytes, the address of an attachment.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// Lowercase hex SHA-256 of a file, read a piece at a time.
///
/// # Errors
///
/// File errors.
pub fn sha256_of_file(path: &Path) -> Result<String> {
    sha256_of_reader(&mut BufReader::new(File::open(path)?)).map(|(sha, _)| sha)
}

/// Lowercase hex SHA-256 of what a reader gives, and its size, read a piece
/// at a time.
///
/// # Errors
///
/// Read errors.
pub fn sha256_of_reader(source: &mut dyn Read) -> Result<(String, u64)> {
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; CHUNK];
    let mut size = 0u64;
    loop {
        let n = source.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        size += n as u64;
    }
    Ok((hex(&hasher.finalize()), size))
}

/// Reads until `buf` is full or the source ends. Returns how much it holds.
fn read_full(source: &mut dyn Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match source.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

/// Content addressed file storage for one vault. A copy is cheap: the sync
/// thread takes one to transfer files without holding the vault.
#[derive(Clone)]
pub struct BlobStore {
    dir: PathBuf,
    key: Option<Zeroizing<[u8; 32]>>,
}

impl std::fmt::Debug for BlobStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlobStore")
            .field("dir", &self.dir)
            .field("encrypted", &self.key.is_some())
            .finish()
    }
}

impl BlobStore {
    /// Opens the store in `dir`. With a key, files are encrypted.
    pub fn new(dir: PathBuf, key: Option<Zeroizing<[u8; 32]>>) -> Self {
        Self { dir, key }
    }

    fn files_key(&self) -> Option<FilesKey> {
        self.key.as_ref().map(|k| app_e2ee::files_key_from(k))
    }

    fn file_name(&self, sha256: &str) -> Result<String> {
        match &self.key {
            None => Ok(sha256.to_owned()),
            Some(key) => {
                let mut mac =
                    Hmac::<Sha256>::new_from_slice(key.as_ref()).map_err(|_| Error::Crypto)?;
                mac.update(sha256.as_bytes());
                Ok(hex(&mac.finalize().into_bytes()))
            }
        }
    }

    /// Two level layout keeps directories small with many files. Anything
    /// but a hash is refused, whoever named it.
    fn path(&self, sha256: &str) -> Result<PathBuf> {
        if !is_sha256_hex(sha256) {
            return Err(Error::Invalid("not a file hash".into()));
        }
        let name = self.file_name(sha256)?;
        Ok(self.dir.join(&name[..2]).join(name))
    }

    /// Whether the file is on this device.
    ///
    /// # Errors
    ///
    /// Crypto errors naming the file.
    pub fn has(&self, sha256: &str) -> Result<bool> {
        Ok(self.path(sha256)?.exists())
    }

    /// Stores bytes and returns their hash. Storing the same content twice
    /// keeps one file.
    ///
    /// # Errors
    ///
    /// File or crypto errors.
    pub fn put(&self, bytes: &[u8]) -> Result<String> {
        let sha256 = sha256_hex(bytes);
        self.put_known(&sha256, &mut &bytes[..])?;
        Ok(sha256)
    }

    /// Stores a file from disk and returns its hash, reading it twice (the
    /// hash first, the encryption needs it) rather than holding it whole.
    ///
    /// # Errors
    ///
    /// File or crypto errors.
    pub fn put_path(&self, path: &Path) -> Result<String> {
        let sha256 = sha256_of_file(path)?;
        self.put_known(&sha256, &mut BufReader::new(File::open(path)?))?;
        Ok(sha256)
    }

    /// Stores content whose hash is known beforehand (a download), checking
    /// it as it goes: content with another hash is not kept.
    ///
    /// # Errors
    ///
    /// [`Error::Corrupt`] when the content does not match the hash, file or
    /// crypto errors.
    pub fn put_known(&self, sha256: &str, source: &mut dyn Read) -> Result<()> {
        let path = self.path(sha256)?;
        if path.exists() {
            return Ok(());
        }
        let parent = path
            .parent()
            .ok_or_else(|| Error::Corrupt("blob path has no parent".into()))?;
        std::fs::create_dir_all(parent)?;
        let suffix = hex(&random_bytes::<6>()?);
        let tmp = path.with_extension(format!("{suffix}.tmp"));
        let written = self.write_to(&tmp, sha256, source);
        match written {
            Ok(()) => {
                std::fs::rename(&tmp, &path)?;
                Ok(())
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                Err(e)
            }
        }
    }

    fn write_to(&self, tmp: &Path, sha256: &str, source: &mut dyn Read) -> Result<()> {
        let mut out = std::io::BufWriter::new(File::create(tmp)?);
        let mut hasher = Sha256::new();
        let mut sealer = self.files_key().map(|k| Sealer::new(&k, sha256));
        if let Some(s) = &sealer {
            out.write_all(&s.header())?;
        }
        let mut current = Zeroizing::new(vec![0u8; CHUNK]);
        let mut next = Zeroizing::new(vec![0u8; CHUNK]);
        let mut len = read_full(source, &mut current)?;
        loop {
            // a chunk is the last when the source has nothing after it
            let next_len = if len == CHUNK {
                read_full(source, &mut next)?
            } else {
                0
            };
            let last = next_len == 0;
            hasher.update(&current[..len]);
            match &mut sealer {
                Some(s) => {
                    out.write_all(&s.seal(&current[..len], last).map_err(|_| Error::Crypto)?)?;
                }
                None => out.write_all(&current[..len])?,
            }
            if last {
                break;
            }
            std::mem::swap(&mut current, &mut next);
            len = next_len;
        }
        if hex(&hasher.finalize()) != sha256 {
            return Err(Error::Corrupt(
                "attachment content does not match its hash".into(),
            ));
        }
        out.into_inner()
            .map_err(|e| Error::Io(e.into_error()))?
            .sync_all()?;
        Ok(())
    }

    /// Reads content by hash, whole, `None` when the file is not on this
    /// device.
    ///
    /// # Errors
    ///
    /// [`Error::Corrupt`] when the file was changed or damaged.
    pub fn get(&self, sha256: &str) -> Result<Option<Vec<u8>>> {
        let Some(mut reader) = self.open(sha256)? else {
            return Ok(None);
        };
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).map_err(corrupt_or_io)?;
        Ok(Some(bytes))
    }

    /// Opens content by hash for reading a piece at a time, `None` when the
    /// file is not on this device. The reader fails at the end if the
    /// content does not match its hash, and on any chunk that does not open.
    ///
    /// # Errors
    ///
    /// File or crypto errors, a file of an unknown format.
    pub fn open(&self, sha256: &str) -> Result<Option<BlobReader>> {
        let path = self.path(sha256)?;
        let mut file = match File::open(&path) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        let len = file.metadata()?.len();
        let source = match self.files_key() {
            None => Source::Plain(BufReader::new(file)),
            Some(files) => {
                let mut first = [0u8; 1];
                read_full(&mut file, &mut first)?;
                match first[0] {
                    FILE_FORMAT => {
                        let mut header = [0u8; FILE_HEADER];
                        header[0] = FILE_FORMAT;
                        if read_full(&mut file, &mut header[1..])? != FILE_HEADER - 1 {
                            return Err(Error::Corrupt("attachment file is cut short".into()));
                        }
                        let opener =
                            Opener::new(&files, sha256, &header).map_err(|_| Error::Crypto)?;
                        Source::Sealed {
                            file: BufReader::new(file),
                            opener,
                            left: len.saturating_sub(FILE_HEADER as u64),
                        }
                    }
                    FORMAT_V1 => Source::Whole(std::io::Cursor::new(self.open_v1(file, sha256)?)),
                    _ => {
                        return Err(Error::Corrupt(
                            "attachment file has an unknown format".into(),
                        ));
                    }
                }
            }
        };
        Ok(Some(BlobReader {
            source,
            chunk: Zeroizing::new(Vec::new()),
            at: 0,
            hasher: Some(Sha256::new()),
            sha256: sha256.to_owned(),
        }))
    }

    /// A file sealed whole before phase 8, its version byte already read.
    fn open_v1(&self, mut file: File, sha256: &str) -> Result<Zeroizing<Vec<u8>>> {
        let key = self.key.as_ref().ok_or(Error::Crypto)?;
        let mut raw = Vec::new();
        file.read_to_end(&mut raw)?;
        if raw.len() < NONCE_LEN {
            return Err(Error::Corrupt("attachment file is cut short".into()));
        }
        let nonce: [u8; NONCE_LEN] = raw[..NONCE_LEN].try_into().map_err(|_| Error::Crypto)?;
        let cipher = XChaCha20Poly1305::new_from_slice(key.as_ref()).map_err(|_| Error::Crypto)?;
        cipher
            .decrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: &raw[NONCE_LEN..],
                    aad: sha256.as_bytes(),
                },
            )
            .map(Zeroizing::new)
            .map_err(|_| Error::Corrupt("attachment file failed its integrity check".into()))
    }

    /// Size of the content, without reading it. `None` when the file is not
    /// on this device.
    ///
    /// # Errors
    ///
    /// File errors, a size no file of its format has.
    pub fn size(&self, sha256: &str) -> Result<Option<u64>> {
        let path = self.path(sha256)?;
        let mut file = match File::open(&path) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        let len = file.metadata()?.len();
        if self.key.is_none() {
            return Ok(Some(len));
        }
        let mut first = [0u8; 1];
        read_full(&mut file, &mut first)?;
        let size = match first[0] {
            FILE_FORMAT => app_e2ee::plain_size(len),
            FORMAT_V1 => len.checked_sub((1 + NONCE_LEN + TAG_LEN) as u64),
            _ => None,
        };
        size.map(Some)
            .ok_or_else(|| Error::Corrupt("attachment file has an unknown size".into()))
    }

    /// Every stored file's hash, for a store without a key, whose files are
    /// named by their hash (an encrypted store hides them). Unfinished
    /// writes are left out.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] for an encrypted store, file errors.
    pub fn names(&self) -> Result<Vec<String>> {
        if self.key.is_some() {
            return Err(Error::Invalid("an encrypted store hides its names".into()));
        }
        let mut names = Vec::new();
        for shard in read_dir_or_empty(&self.dir)? {
            for file in read_dir_or_empty(&shard)? {
                let name = file
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if name.len() == 64 && name.chars().all(|c| c.is_ascii_hexdigit()) {
                    names.push(name);
                }
            }
        }
        Ok(names)
    }

    /// Deletes files no attachment refers to anymore, and writes left over
    /// by an interrupted store. Returns how many were removed.
    ///
    /// # Errors
    ///
    /// File errors.
    pub fn remove_unreferenced(&self, referenced: &HashSet<String>) -> Result<usize> {
        let keep: HashSet<String> = referenced
            .iter()
            .map(|sha| self.file_name(sha))
            .collect::<Result<_>>()?;
        let mut removed = 0;
        for shard in read_dir_or_empty(&self.dir)? {
            for file in read_dir_or_empty(&shard)? {
                let name = file
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                // a write under way: left alone unless a day old (a crash)
                let fresh = Path::new(&name)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("tmp"))
                    && std::fs::metadata(&file)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .is_some_and(|age| age < std::time::Duration::from_hours(24));
                if !keep.contains(&name) && !fresh {
                    std::fs::remove_file(&file)?;
                    removed += 1;
                }
            }
        }
        Ok(removed)
    }
}

/// An io error from a [`BlobReader`] back into the crate's error.
fn corrupt_or_io(err: std::io::Error) -> Error {
    if err.kind() == std::io::ErrorKind::InvalidData {
        Error::Corrupt(err.to_string())
    } else {
        Error::Io(err)
    }
}

enum Source {
    Plain(BufReader<File>),
    Whole(std::io::Cursor<Zeroizing<Vec<u8>>>),
    Sealed {
        file: BufReader<File>,
        opener: Opener,
        /// Sealed bytes not read yet.
        left: u64,
    },
}

/// Content read a piece at a time, see [`BlobStore::open`].
pub struct BlobReader {
    source: Source,
    /// The opened chunk being handed out.
    chunk: Zeroizing<Vec<u8>>,
    at: usize,
    /// Until the end is reached and checked.
    hasher: Option<Sha256>,
    sha256: String,
}

fn invalid(what: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, what.to_owned())
}

impl BlobReader {
    /// The next piece of content, empty at the end.
    fn next_piece(&mut self) -> std::io::Result<()> {
        self.at = 0;
        self.chunk.clear();
        match &mut self.source {
            Source::Plain(file) => {
                self.chunk.resize(CHUNK, 0);
                let n = read_full(file, &mut self.chunk)?;
                self.chunk.truncate(n);
            }
            Source::Whole(cursor) => {
                self.chunk.resize(CHUNK, 0);
                let n = read_full(cursor, &mut self.chunk)?;
                self.chunk.truncate(n);
            }
            Source::Sealed { file, opener, left } => {
                if *left == 0 {
                    return Ok(());
                }
                let take = (*left).min(SEALED_CHUNK as u64);
                let last = take == *left;
                let mut sealed = vec![0u8; usize::try_from(take).unwrap_or(SEALED_CHUNK)];
                if read_full(file, &mut sealed)? != sealed.len() {
                    return Err(invalid("attachment file is cut short"));
                }
                *left -= take;
                let plain = opener
                    .open(&sealed, last)
                    .map_err(|_| invalid("attachment file failed its integrity check"))?;
                self.chunk.extend_from_slice(&plain);
                if last && self.chunk.is_empty() {
                    // an empty file: its one chunk is read, the end follows
                    return Ok(());
                }
            }
        }
        Ok(())
    }
}

impl Read for BlobReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.at == self.chunk.len() {
            self.next_piece()?;
            if self.chunk.is_empty() {
                // the end: the content must be the one asked for
                if let Some(hasher) = self.hasher.take()
                    && hex(&hasher.finalize()) != self.sha256
                {
                    return Err(invalid("attachment content does not match its hash"));
                }
                return Ok(0);
            }
            if let Some(hasher) = &mut self.hasher {
                hasher.update(&self.chunk);
            }
        }
        let n = buf.len().min(self.chunk.len() - self.at);
        buf[..n].copy_from_slice(&self.chunk[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

fn read_dir_or_empty(dir: &Path) -> Result<Vec<PathBuf>> {
    match std::fs::read_dir(dir) {
        Ok(entries) => Ok(entries.filter_map(|e| e.ok().map(|e| e.path())).collect()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(err) => Err(err.into()),
    }
}

impl AttachmentSource for BlobStore {
    fn read(&self, sha256: &str) -> Result<Option<Vec<u8>>> {
        self.get(sha256)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stores() -> (tempfile::TempDir, BlobStore, BlobStore) {
        let dir = tempfile::tempdir().unwrap();
        let plain = BlobStore::new(dir.path().join("plain"), None);
        let sealed = BlobStore::new(dir.path().join("sealed"), Some(Zeroizing::new([7u8; 32])));
        (dir, plain, sealed)
    }

    /// A hash from a synced change or a backup
    /// that is a path, absolute or not, reads nothing outside the store, and
    /// a short one does not panic.
    #[test]
    fn only_hashes_become_paths() {
        let (dir, plain, sealed) = stores();
        let outside = dir.path().join("id_ed25519");
        std::fs::write(&outside, b"a private key").unwrap();
        let absolute = outside.to_string_lossy().into_owned();
        let upper = sha256_hex(b"x").to_uppercase();
        for store in [&plain, &sealed] {
            for bad in [
                absolute.as_str(),
                "../../id_ed25519",
                "ab",
                "",
                "é",
                upper.as_str(),
            ] {
                assert!(store.has(bad).is_err(), "{bad}");
                assert!(store.get(bad).is_err(), "{bad}");
                assert!(store.size(bad).is_err(), "{bad}");
            }
        }
        assert!(is_sha256_hex(&sha256_hex(b"x")));
    }

    #[test]
    fn round_trips_and_deduplicates() {
        let (_dir, plain, sealed) = stores();
        for store in [&plain, &sealed] {
            let a = store.put(b"tomato vines").unwrap();
            assert_eq!(store.put(b"tomato vines").unwrap(), a);
            assert_eq!(store.get(&a).unwrap().unwrap(), b"tomato vines");
            assert_eq!(store.size(&a).unwrap(), Some(12));
            assert!(store.has(&a).unwrap());
            assert_eq!(store.get(&sha256_hex(b"other")).unwrap(), None);
            assert_eq!(store.size(&sha256_hex(b"other")).unwrap(), None);
        }
    }

    #[test]
    fn files_of_every_size_stream_through() {
        let (dir, plain, sealed) = stores();
        for len in [0, 1, CHUNK - 1, CHUNK, CHUNK + 1, 3 * CHUNK + 5] {
            let content: Vec<u8> = (0..len).map(|i| u8::try_from(i % 253).unwrap()).collect();
            let source = dir.path().join(format!("in-{len}"));
            std::fs::write(&source, &content).unwrap();
            for store in [&plain, &sealed] {
                let sha = store.put_path(&source).unwrap();
                assert_eq!(sha, sha256_hex(&content));
                assert_eq!(store.size(&sha).unwrap(), Some(len as u64), "{len}");
                let mut read = Vec::new();
                // small reads cross chunk boundaries
                let mut reader = store.open(&sha).unwrap().unwrap();
                let mut buf = [0u8; 1000];
                loop {
                    let n = reader.read(&mut buf).unwrap();
                    if n == 0 {
                        break;
                    }
                    read.extend_from_slice(&buf[..n]);
                }
                assert_eq!(read, content, "{len}");
            }
        }
    }

    #[test]
    fn content_with_another_hash_is_not_kept() {
        let (_dir, plain, sealed) = stores();
        for store in [&plain, &sealed] {
            let wanted = sha256_hex(b"what was asked");
            assert!(matches!(
                store.put_known(&wanted, &mut &b"something else"[..]),
                Err(Error::Corrupt(_))
            ));
            assert!(!store.has(&wanted).unwrap());
            // nothing left behind
            assert_eq!(store.remove_unreferenced(&HashSet::new()).unwrap(), 0);
        }
    }

    #[test]
    fn encrypted_files_hide_content_and_name() {
        let (_dir, _, sealed) = stores();
        let sha = sealed.put(b"secret picture").unwrap();
        let path = sealed.path(&sha).unwrap();
        assert!(!path.to_string_lossy().contains(&sha));
        let raw = std::fs::read(path).unwrap();
        assert!(!raw.windows(6).any(|w| w == b"secret"));
    }

    #[test]
    fn tampering_is_detected() {
        let (_dir, plain, sealed) = stores();
        for store in [&plain, &sealed] {
            let sha = store.put(&vec![5u8; CHUNK * 2]).unwrap();
            let path = store.path(&sha).unwrap();
            let mut raw = std::fs::read(&path).unwrap();
            raw[CHUNK + 40] ^= 1;
            std::fs::write(&path, &raw).unwrap();
            assert!(matches!(store.get(&sha), Err(Error::Corrupt(_))));
            // cut short
            raw.truncate(raw.len() - 1);
            std::fs::write(&path, &raw).unwrap();
            assert!(store.get(&sha).is_err());
        }
    }

    #[test]
    fn files_sealed_whole_before_still_open() {
        let (_dir, _, sealed) = stores();
        let content = b"an old picture";
        let sha = sha256_hex(content);
        // as phase 2 wrote them
        let nonce = [3u8; NONCE_LEN];
        let cipher = XChaCha20Poly1305::new_from_slice(&[7u8; 32]).unwrap();
        let body = cipher
            .encrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: content,
                    aad: sha.as_bytes(),
                },
            )
            .unwrap();
        let mut raw = vec![FORMAT_V1];
        raw.extend_from_slice(&nonce);
        raw.extend_from_slice(&body);
        let path = sealed.path(&sha).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, raw).unwrap();
        assert_eq!(sealed.get(&sha).unwrap().unwrap(), content);
        assert_eq!(sealed.size(&sha).unwrap(), Some(content.len() as u64));
    }

    #[test]
    fn removes_only_unreferenced_files() {
        let (_dir, _, sealed) = stores();
        let keep = sealed.put(b"keep").unwrap();
        sealed.put(b"drop").unwrap();
        assert_eq!(
            sealed
                .remove_unreferenced(&HashSet::from([keep.clone()]))
                .unwrap(),
            1
        );
        assert!(sealed.get(&keep).unwrap().is_some());
    }
}
