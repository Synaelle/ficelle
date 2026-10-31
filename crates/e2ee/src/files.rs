// SPDX-License-Identifier: AGPL-3.0-or-later

//! Attachment files, sealed as a stream so a file of any size is encrypted,
//! sent, received and checked a piece at a time.
//!
//! ```text
//! version (1 byte) | nonce prefix (19 bytes) | chunk 0 | chunk 1 | ... | last chunk
//! ```
//!
//! Each chunk holds 64 KB of the file (the last one less, an empty file one
//! empty chunk) sealed with XChaCha20-Poly1305 in the STREAM construction
//! (`aead-stream`, a 32 bit big endian counter and a last chunk flag in the
//! nonce): a file cut short, chunks reordered or taken from another file do
//! not open. The file's SHA-256 is the associated data of every chunk.
//!
//! Keys: a space's files key comes from the account master key (personal
//! space) or the space key of an epoch (shared space). Each file's key and
//! nonce prefix derive from it and the file's hash, so the same file in the
//! same space always seals to the same bytes: one copy on the server, and an
//! upload cut off resumes. A key is only ever used for one content, so the
//! fixed nonce never encrypts two different texts. The address the server
//! files it under derives from the same key, so the server learns neither
//! the hash nor the content.

use aead_stream::aead::Payload;
use aead_stream::{DecryptorBE32, EncryptorBE32, NewStream as _, StreamBE32, StreamPrimitive as _};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305};
use zeroize::Zeroizing;

use crate::keys::MasterKey;
use crate::share::SpaceKey;
use crate::sync::derive_with;
use crate::{Error, Result};

/// First byte of a sealed file.
pub const FORMAT: u8 = 2;
/// Plain bytes in each chunk but the last.
pub const CHUNK: usize = 64 * 1024;
const TAG: usize = 16;
/// Bytes of a sealed chunk but the last.
pub const SEALED_CHUNK: usize = CHUNK + TAG;
const PREFIX: usize = 19;
/// Bytes before the first chunk.
pub const HEADER: usize = 1 + PREFIX;

const FILES_KEY: &[u8] = b"notes e2ee v1 files";
const FILE_KEY: &[u8] = b"notes e2ee v1 file key ";
const FILE_PREFIX: &[u8] = b"notes e2ee v1 file nonce ";
const FILE_ADDRESS: &[u8] = b"notes e2ee v1 file address ";

/// The key a space's files derive from.
pub type FilesKey = Zeroizing<[u8; 32]>;

/// Files key of an account's personal space.
pub fn files_key(master: &MasterKey) -> FilesKey {
    derive_with(master.as_bytes(), FILES_KEY)
}

/// Files key of a shared space, for one epoch.
pub fn space_files_key(space_key: &SpaceKey) -> FilesKey {
    derive_with(space_key.as_slice(), FILES_KEY)
}

/// Files key from any 32 byte secret, for a vault's own encrypted folder.
pub fn files_key_from(secret: &[u8; 32]) -> FilesKey {
    derive_with(secret, FILES_KEY)
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
            // writing to a String cannot fail
            let _ = write!(out, "{b:02x}");
            out
        })
}

/// Where the server keeps a file: 64 hex characters that say nothing of
/// the content without the key.
pub fn file_address(files: &FilesKey, sha256: &str) -> String {
    hex(derive_with(
        files.as_slice(),
        &[FILE_ADDRESS, sha256.as_bytes()].concat(),
    )
    .as_slice())
}

/// Size of a file of `plain` bytes once sealed.
pub fn sealed_size(plain: u64) -> u64 {
    let chunk = CHUNK as u64;
    let chunks = plain.div_ceil(chunk).max(1);
    HEADER as u64 + plain + chunks * TAG as u64
}

/// Size of the file a sealed file of `sealed` bytes holds, `None` when no
/// file seals to that size.
pub fn plain_size(sealed: u64) -> Option<u64> {
    let body = sealed.checked_sub(HEADER as u64)?;
    let full = SEALED_CHUNK as u64;
    // every chunk but the last is full, the last holds at least its tag
    let chunks = body.div_ceil(full).max(1);
    let plain = body.checked_sub(chunks * TAG as u64)?;
    (sealed_size(plain) == sealed).then_some(plain)
}

fn file_secrets(files: &FilesKey, sha256: &str) -> (Zeroizing<[u8; 32]>, [u8; PREFIX]) {
    let key = derive_with(files.as_slice(), &[FILE_KEY, sha256.as_bytes()].concat());
    let long = derive_with(files.as_slice(), &[FILE_PREFIX, sha256.as_bytes()].concat());
    let mut prefix = [0u8; PREFIX];
    prefix.copy_from_slice(&long[..PREFIX]);
    (key, prefix)
}

fn stream(key: &[u8; 32], prefix: &[u8; PREFIX]) -> StreamBE32<XChaCha20Poly1305> {
    let aead = XChaCha20Poly1305::new(key.into());
    StreamBE32::from_aead(aead, prefix.into())
}

/// Seals a file chunk by chunk.
pub struct Sealer {
    encryptor: Option<EncryptorBE32<XChaCha20Poly1305>>,
    sha256: String,
    prefix: [u8; PREFIX],
}

impl Sealer {
    /// The sealer of a file in a space.
    pub fn new(files: &FilesKey, sha256: &str) -> Self {
        let (key, prefix) = file_secrets(files, sha256);
        Self {
            encryptor: Some(stream(&key, &prefix).encryptor()),
            sha256: sha256.to_owned(),
            prefix,
        }
    }

    /// The bytes that go before the first chunk.
    pub fn header(&self) -> [u8; HEADER] {
        let mut out = [0u8; HEADER];
        out[0] = FORMAT;
        out[1..].copy_from_slice(&self.prefix);
        out
    }

    /// Seals the next chunk, `CHUNK` bytes unless it is the last.
    ///
    /// # Errors
    ///
    /// [`Error::Malformed`] for a chunk of the wrong size or one after the
    /// last.
    pub fn seal(&mut self, chunk: &[u8], last: bool) -> Result<Vec<u8>> {
        if chunk.len() > CHUNK || (!last && chunk.len() != CHUNK) {
            return Err(Error::Malformed("file chunk"));
        }
        let payload = Payload {
            msg: chunk,
            aad: self.sha256.as_bytes(),
        };
        let sealed = if last {
            self.encryptor
                .take()
                .ok_or(Error::Malformed("file chunk"))?
                .encrypt_last(payload)
        } else {
            self.encryptor
                .as_mut()
                .ok_or(Error::Malformed("file chunk"))?
                .encrypt_next(payload)
        };
        sealed.map_err(|_| Error::Malformed("file chunk"))
    }
}

/// Opens a sealed file chunk by chunk.
pub struct Opener {
    decryptor: Option<DecryptorBE32<XChaCha20Poly1305>>,
    sha256: String,
}

impl Opener {
    /// The opener of a file in a space, from the header it was sealed with.
    ///
    /// # Errors
    ///
    /// [`Error::Malformed`] for a header of another format.
    pub fn new(files: &FilesKey, sha256: &str, header: &[u8]) -> Result<Self> {
        if header.len() != HEADER || header[0] != FORMAT {
            return Err(Error::Malformed("file header"));
        }
        let (key, _) = file_secrets(files, sha256);
        let mut prefix = [0u8; PREFIX];
        prefix.copy_from_slice(&header[1..]);
        Ok(Self {
            decryptor: Some(stream(&key, &prefix).decryptor()),
            sha256: sha256.to_owned(),
        })
    }

    /// Opens the next chunk, `SEALED_CHUNK` bytes unless it is the last.
    ///
    /// # Errors
    ///
    /// [`Error::WrongKey`] when it does not open: another key or file, a
    /// chunk changed, missing, moved, or the file cut short.
    pub fn open(&mut self, sealed: &[u8], last: bool) -> Result<Zeroizing<Vec<u8>>> {
        if sealed.len() > SEALED_CHUNK || (!last && sealed.len() != SEALED_CHUNK) {
            return Err(Error::Malformed("file chunk"));
        }
        let payload = Payload {
            msg: sealed,
            aad: self.sha256.as_bytes(),
        };
        let plain = if last {
            self.decryptor
                .take()
                .ok_or(Error::Malformed("file chunk"))?
                .decrypt_last(payload)
        } else {
            self.decryptor
                .as_mut()
                .ok_or(Error::Malformed("file chunk"))?
                .decrypt_next(payload)
        };
        plain.map(Zeroizing::new).map_err(|_| Error::WrongKey)
    }
}

/// Seals and opens chunks by their position, for a file that arrives in
/// parts or is read from the middle. The bytes are the same as a
/// [`Sealer`]'s.
pub struct FileCipher {
    stream: StreamBE32<XChaCha20Poly1305>,
    sha256: String,
    prefix: [u8; PREFIX],
}

impl FileCipher {
    /// The cipher of a file in a space.
    pub fn new(files: &FilesKey, sha256: &str) -> Self {
        let (key, prefix) = file_secrets(files, sha256);
        Self {
            stream: stream(&key, &prefix),
            sha256: sha256.to_owned(),
            prefix,
        }
    }

    /// The bytes that go before the first chunk.
    pub fn header(&self) -> [u8; HEADER] {
        let mut out = [0u8; HEADER];
        out[0] = FORMAT;
        out[1..].copy_from_slice(&self.prefix);
        out
    }

    /// Seals chunk number `index`, `CHUNK` bytes unless it is the last.
    ///
    /// # Errors
    ///
    /// [`Error::Malformed`] for a chunk of the wrong size.
    pub fn seal_at(&self, index: u32, chunk: &[u8], last: bool) -> Result<Vec<u8>> {
        if chunk.len() > CHUNK || (!last && chunk.len() != CHUNK) {
            return Err(Error::Malformed("file chunk"));
        }
        self.stream
            .encrypt(
                index,
                last,
                Payload {
                    msg: chunk,
                    aad: self.sha256.as_bytes(),
                },
            )
            .map_err(|_| Error::Malformed("file chunk"))
    }

    /// Opens chunk number `index`.
    ///
    /// # Errors
    ///
    /// [`Error::WrongKey`] when it does not open.
    pub fn open_at(&self, index: u32, sealed: &[u8], last: bool) -> Result<Zeroizing<Vec<u8>>> {
        if sealed.len() > SEALED_CHUNK || (!last && sealed.len() != SEALED_CHUNK) {
            return Err(Error::Malformed("file chunk"));
        }
        self.stream
            .decrypt(
                index,
                last,
                Payload {
                    msg: sealed,
                    aad: self.sha256.as_bytes(),
                },
            )
            .map(Zeroizing::new)
            .map_err(|_| Error::WrongKey)
    }
}

/// Seals a whole file held in memory.
///
/// # Errors
///
/// None in practice: chunks are cut to size here.
pub fn seal_file(files: &FilesKey, sha256: &str, plain: &[u8]) -> Result<Vec<u8>> {
    let mut sealer = Sealer::new(files, sha256);
    let mut out = Vec::with_capacity(usize::try_from(sealed_size(plain.len() as u64)).unwrap_or(0));
    out.extend_from_slice(&sealer.header());
    let count = plain.len().div_ceil(CHUNK).max(1);
    for (i, chunk) in plain.chunks(CHUNK).enumerate() {
        out.extend(sealer.seal(chunk, i + 1 == count)?);
    }
    if plain.is_empty() {
        out.extend(sealer.seal(&[], true)?);
    }
    Ok(out)
}

/// Opens a whole sealed file held in memory.
///
/// # Errors
///
/// As [`Opener::open`], and [`Error::Malformed`] for a size no file seals
/// to.
pub fn open_file(files: &FilesKey, sha256: &str, sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    let size = plain_size(sealed.len() as u64).ok_or(Error::Malformed("sealed file"))?;
    let mut opener = Opener::new(files, sha256, &sealed[..HEADER])?;
    let mut out = Zeroizing::new(Vec::with_capacity(usize::try_from(size).unwrap_or(0)));
    let body = &sealed[HEADER..];
    let count = body.len().div_ceil(SEALED_CHUNK).max(1);
    for (i, chunk) in body.chunks(SEALED_CHUNK).enumerate() {
        out.extend_from_slice(&opener.open(chunk, i + 1 == count)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

    fn key(byte: u8) -> FilesKey {
        files_key_from(&[byte; 32])
    }

    #[test]
    fn sizes_round_trip_at_every_boundary() {
        for plain in [0, 1, CHUNK - 1, CHUNK, CHUNK + 1, 3 * CHUNK, 3 * CHUNK + 7] {
            let sealed = seal_file(&key(1), SHA, &vec![7u8; plain]).unwrap();
            assert_eq!(sealed.len() as u64, sealed_size(plain as u64), "{plain}");
            assert_eq!(
                plain_size(sealed.len() as u64),
                Some(plain as u64),
                "{plain}"
            );
            assert_eq!(open_file(&key(1), SHA, &sealed).unwrap().len(), plain);
        }
        assert_eq!(plain_size(HEADER as u64 + 3), None);
    }

    #[test]
    fn the_same_file_in_the_same_space_seals_the_same() {
        let plain = vec![42u8; CHUNK * 2 + 5];
        let a = seal_file(&key(1), SHA, &plain).unwrap();
        assert_eq!(a, seal_file(&key(1), SHA, &plain).unwrap());
        assert_ne!(a, seal_file(&key(2), SHA, &plain).unwrap());
        assert_eq!(file_address(&key(1), SHA), file_address(&key(1), SHA));
        assert_ne!(file_address(&key(1), SHA), file_address(&key(2), SHA));
        assert_eq!(file_address(&key(1), SHA).len(), 64);
        assert!(!a.windows(8).any(|w| w == [42u8; 8]));
    }

    #[test]
    fn a_changed_cut_or_reordered_file_does_not_open() {
        let plain: Vec<u8> = (0..CHUNK * 3)
            .map(|i| u8::try_from(i % 251).unwrap())
            .collect();
        let sealed = seal_file(&key(1), SHA, &plain).unwrap();
        // a flipped bit
        let mut bad = sealed.clone();
        bad[HEADER + 10] ^= 1;
        assert!(open_file(&key(1), SHA, &bad).is_err());
        // cut after a whole chunk: the new end is not a last chunk
        let cut = &sealed[..HEADER + SEALED_CHUNK * 2];
        assert!(open_file(&key(1), SHA, cut).is_err());
        // two chunks swapped
        let mut swapped = sealed[..HEADER].to_vec();
        swapped.extend_from_slice(&sealed[HEADER + SEALED_CHUNK..HEADER + 2 * SEALED_CHUNK]);
        swapped.extend_from_slice(&sealed[HEADER..HEADER + SEALED_CHUNK]);
        swapped.extend_from_slice(&sealed[HEADER + 2 * SEALED_CHUNK..]);
        assert!(open_file(&key(1), SHA, &swapped).is_err());
        // another key, another file's hash
        assert!(open_file(&key(2), SHA, &sealed).is_err());
        assert!(open_file(&key(1), &SHA.replace('9', "8"), &sealed).is_err());
        assert_eq!(*open_file(&key(1), SHA, &sealed).unwrap(), plain);
    }

    #[test]
    fn chunks_of_the_wrong_size_are_refused() {
        let mut sealer = Sealer::new(&key(1), SHA);
        assert!(sealer.seal(&[0u8; 10], false).is_err());
        assert!(sealer.seal(&vec![0u8; CHUNK + 1], true).is_err());
        sealer.seal(&[0u8; 10], true).unwrap();
        assert!(
            sealer.seal(&[0u8; 10], true).is_err(),
            "nothing after the last"
        );
    }

    #[test]
    fn chunks_by_position_match_the_stream() {
        let plain: Vec<u8> = (0..CHUNK * 2 + 9)
            .map(|i| u8::try_from(i % 249).unwrap())
            .collect();
        let sealed = seal_file(&key(3), SHA, &plain).unwrap();
        let cipher = FileCipher::new(&key(3), SHA);
        assert_eq!(cipher.header(), sealed[..HEADER]);
        let mut by_parts = cipher.header().to_vec();
        for (i, chunk) in plain.chunks(CHUNK).enumerate() {
            let index = u32::try_from(i).unwrap();
            by_parts.extend(cipher.seal_at(index, chunk, i == 2).unwrap());
        }
        assert_eq!(by_parts, sealed);
        // the middle chunk read alone, and not as another position
        let middle = &sealed[HEADER + SEALED_CHUNK..HEADER + 2 * SEALED_CHUNK];
        assert_eq!(
            *cipher.open_at(1, middle, false).unwrap(),
            plain[CHUNK..2 * CHUNK]
        );
        assert!(cipher.open_at(0, middle, false).is_err());
        assert!(cipher.open_at(1, middle, true).is_err());
    }

    #[test]
    fn personal_and_shared_files_keys_differ() {
        let master = MasterKey::generate();
        let space = crate::share::new_space_key();
        assert_ne!(*files_key(&master), *space_files_key(&space));
        assert_ne!(*files_key(&master), *master.as_bytes());
    }
}
