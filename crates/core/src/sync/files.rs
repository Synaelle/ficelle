// SPDX-License-Identifier: AGPL-3.0-or-later

//! Attachment files with the server, after the notes' round: upload what
//! this device attached, fetch what its notes use and it lacks, let go of
//! what nothing uses any more. A file in a personal e2ee space or an e2ee
//! shared space is sealed here (see [`app_e2ee::seal_file`]), a standard
//! one goes as it is and the server seals it.
//!
//! Uploads resume where the server says they stand: sealing is
//! deterministic, so the bytes past that point are the same. Downloads are
//! checked against the file's hash before they are kept. A file is only let
//! go when everything of its space is on the server, and the server refuses
//! if the space moved on meanwhile (another device may use the file).

use std::io::Read;

use app_e2ee::{CHUNK, FILE_HEADER, FilesKey, Opener, SEALED_CHUNK, Sealer};
use app_protocol::error_code;
use app_protocol::files::{FileState, FileStatus};

use super::engine::{Access, Remote, RemoteError, SyncError};
use crate::blobs::BlobStore;
use crate::error::Error;
use crate::store::{FileKeying, FileUse};

/// A file call that failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FileError {
    /// The server could not be used, as for the notes.
    #[error(transparent)]
    Remote(#[from] RemoteError),
    /// The server does not have it (not uploaded yet, or let go).
    #[error("the server does not have this file")]
    Missing,
    /// The server will not take it: an error code such as
    /// [`error_code::FILE_TOO_LARGE`].
    #[error("the server refused the file: {0}")]
    Refused(String),
}

/// One part of an upload.
#[derive(Debug, Clone, Copy)]
pub struct FilePart<'a> {
    /// Where it starts in the file.
    pub start: u64,
    /// The file's whole size.
    pub total: u64,
    /// Its bytes.
    pub bytes: &'a [u8],
    /// Standard files: the name and type it is attached under, sent with
    /// the first part for the server's type rules.
    pub declared: Option<(&'a str, &'a str)>,
}

/// What a files round did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FilesRound {
    /// Files sent.
    pub uploaded: u32,
    /// Files fetched.
    pub fetched: u32,
    /// Files let go.
    pub released: u32,
    /// Files the server refused (too large, a type not allowed).
    pub refused: u32,
    /// A space's owner has no room left.
    pub quota_full: bool,
}

/// Reads a plain stream as the sealed file of [`app_e2ee::seal_file`].
struct Sealing<R> {
    source: R,
    sealer: Sealer,
    out: Vec<u8>,
    at: usize,
    /// The next chunk, read ahead to know which one is the last.
    next: Option<Vec<u8>>,
    done: bool,
}

fn read_chunk(source: &mut dyn Read, size: usize) -> std::io::Result<Vec<u8>> {
    let mut buf = vec![0u8; size];
    let mut filled = 0;
    while filled < size {
        match source.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    buf.truncate(filled);
    Ok(buf)
}

impl<R: Read> Sealing<R> {
    fn new(source: R, files: &FilesKey, sha256: &str) -> Self {
        let sealer = Sealer::new(files, sha256);
        Self {
            out: sealer.header().to_vec(),
            sealer,
            source,
            at: 0,
            next: None,
            done: false,
        }
    }
}

impl<R: Read> Read for Sealing<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.at == self.out.len() {
            if self.done {
                return Ok(0);
            }
            let current = match self.next.take() {
                Some(chunk) => chunk,
                None => read_chunk(&mut self.source, CHUNK)?,
            };
            let last = if current.len() < CHUNK {
                true
            } else {
                let next = read_chunk(&mut self.source, CHUNK)?;
                let empty = next.is_empty();
                self.next = Some(next);
                empty
            };
            self.out = self
                .sealer
                .seal(&current, last)
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            self.at = 0;
            self.done = last;
        }
        let n = buf.len().min(self.out.len() - self.at);
        buf[..n].copy_from_slice(&self.out[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

/// Reads a sealed stream as the plain file, failing on anything that does
/// not open.
struct Opening<R> {
    source: R,
    files: FilesKey,
    sha256: String,
    opener: Option<Opener>,
    out: Vec<u8>,
    at: usize,
    next: Option<Vec<u8>>,
    done: bool,
}

impl<R: Read> Opening<R> {
    fn new(source: R, files: FilesKey, sha256: &str) -> Self {
        Self {
            source,
            files,
            sha256: sha256.to_owned(),
            opener: None,
            out: Vec::new(),
            at: 0,
            next: None,
            done: false,
        }
    }
}

fn damaged(what: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, what.to_owned())
}

impl<R: Read> Read for Opening<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        while self.at == self.out.len() {
            if self.done {
                return Ok(0);
            }
            if self.opener.is_none() {
                let header = read_chunk(&mut self.source, FILE_HEADER)?;
                if header.len() != FILE_HEADER {
                    return Err(damaged("the file is cut short"));
                }
                self.opener = Some(
                    Opener::new(&self.files, &self.sha256, &header)
                        .map_err(|_| damaged("the file has an unknown format"))?,
                );
            }
            let current = match self.next.take() {
                Some(chunk) => chunk,
                None => read_chunk(&mut self.source, SEALED_CHUNK)?,
            };
            let last = if current.len() < SEALED_CHUNK {
                true
            } else {
                let next = read_chunk(&mut self.source, SEALED_CHUNK)?;
                let empty = next.is_empty();
                self.next = Some(next);
                empty
            };
            let opener = self.opener.as_mut().ok_or_else(|| damaged("no header"))?;
            self.out = opener
                .open(&current, last)
                .map_err(|_| damaged("the file does not open"))?
                .to_vec();
            self.at = 0;
            self.done = last;
        }
        let n = buf.len().min(self.out.len() - self.at);
        buf[..n].copy_from_slice(&self.out[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

/// Where a file lives on the server for its space, and how it travels.
fn address(keying: &FileKeying, sha256: &str) -> Option<String> {
    match keying {
        FileKeying::Plain => Some(sha256.to_owned()),
        FileKeying::Sealed(files) => Some(app_e2ee::file_address(files, sha256)),
        FileKeying::Waiting => None,
    }
}

fn space_of(space: &str) -> Option<&str> {
    (!space.is_empty()).then_some(space)
}

/// Sends one file, from where the server says it stands. Returns whether
/// any bytes went: a file the space holds, or takes back, sends none.
fn upload(
    remote: &mut impl Remote,
    blobs: &BlobStore,
    file: &FileUse,
    keying: &FileKeying,
    address: &str,
    part_bytes: u64,
) -> Result<bool, FileError> {
    let space = space_of(&file.space);
    let fail = |e: &dyn std::fmt::Display| FileError::Refused(format!("local: {e}"));
    let Some(plain) = blobs.size(&file.sha256).map_err(|e| fail(&e))? else {
        return Ok(false);
    };
    let total = match keying {
        FileKeying::Sealed(_) => app_e2ee::sealed_size(plain),
        _ => plain,
    };
    let state: FileState = remote.file_state(space, address)?;
    let mut start = match state.status {
        FileStatus::Complete => return Ok(false),
        FileStatus::Kept => return remote.keep_file(space, address).map(|()| false),
        FileStatus::Partial if state.size == total => state.received,
        _ => 0,
    };
    let source = blobs
        .open(&file.sha256)
        .map_err(|e| fail(&e))?
        .ok_or(FileError::Missing)?;
    let mut reader: Box<dyn Read> = match keying {
        FileKeying::Sealed(files) => Box::new(Sealing::new(source, files, &file.sha256)),
        _ => Box::new(source),
    };
    std::io::copy(&mut (&mut reader).take(start), &mut std::io::sink()).map_err(|e| fail(&e))?;
    let declared =
        matches!(keying, FileKeying::Plain).then_some((file.name.as_str(), file.mime.as_str()));
    loop {
        let want = usize::try_from(part_bytes.min(total - start)).unwrap_or(usize::MAX);
        let bytes = read_chunk(&mut reader, want).map_err(|e| fail(&e))?;
        if bytes.len() != want {
            return Err(fail(&"the file changed while it was sent"));
        }
        let answer = remote.put_file(
            space,
            address,
            FilePart {
                start,
                total,
                bytes: &bytes,
                declared: declared.filter(|_| start == 0),
            },
        )?;
        start += bytes.len() as u64;
        if answer.status == FileStatus::Complete || start >= total {
            return Ok(true);
        }
    }
}

/// Fetches one file into the vault.
fn fetch_one(
    remote: &mut impl Remote,
    blobs: &BlobStore,
    space: &str,
    sha256: &str,
    keying: FileKeying,
    address: &str,
) -> Result<(), FileError> {
    let source = remote.get_file(space_of(space), address)?;
    let mut reader: Box<dyn Read> = match keying {
        FileKeying::Sealed(files) => Box::new(Opening::new(source, files, sha256)),
        _ => source,
    };
    blobs
        .put_known(sha256, &mut reader)
        .map_err(|e| FileError::Refused(format!("local: {e}")))
}

/// Fetches a file from its own space, else from the spaces notes moved
/// from held it in (it stays there for the retention time). Returns where
/// it came from.
fn fetch_somewhere(
    store: &impl Access,
    remote: &mut impl Remote,
    blobs: &BlobStore,
    file: &FileUse,
) -> Result<(), FileError> {
    let local = |e: Error| FileError::Refused(format!("local: {e}"));
    let keying = store
        .with(|s| s.file_keying(&file.space, file.epoch))
        .map_err(local)?;
    let mut last = FileError::Missing;
    if let Some(address) = address(&keying, &file.sha256) {
        match fetch_one(remote, blobs, &file.space, &file.sha256, keying, &address) {
            // not marked as held: the next upload pass asks the server,
            // which takes back a file let go meanwhile
            Ok(()) => {
                store
                    .with(|s| s.file_arrived(&file.sha256))
                    .map_err(local)?;
                return Ok(());
            }
            Err(FileError::Remote(e)) => return Err(FileError::Remote(e)),
            Err(e) => last = e,
        }
    }
    for (space, epoch) in store
        .with(|s| s.file_sources(&file.sha256))
        .map_err(local)?
    {
        if space == file.space {
            continue;
        }
        let Ok(keying) = store.with(|s| s.file_keying(&space, epoch)) else {
            continue;
        };
        let Some(address) = address(&keying, &file.sha256) else {
            continue;
        };
        match fetch_one(remote, blobs, &space, &file.sha256, keying, &address) {
            Ok(()) => {
                store
                    .with(|s| s.file_arrived(&file.sha256))
                    .map_err(local)?;
                return Ok(());
            }
            Err(FileError::Remote(e)) => return Err(FileError::Remote(e)),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// Fetches a file a note uses, now: a large one the round left for later,
/// once the person opens it. Does nothing when it is here already.
///
/// # Errors
///
/// [`FileError::Missing`] when no note here uses it or the server does not
/// have it, the server's errors.
pub fn fetch(
    store: &impl Access,
    blobs: &BlobStore,
    remote: &mut impl Remote,
    sha256: &str,
) -> Result<(), FileError> {
    if blobs.has(sha256).unwrap_or(false) {
        return Ok(());
    }
    let refused = |e: Error| FileError::Refused(format!("local: {e}"));
    let uses: Vec<FileUse> = store
        .with(|s| s.files_in_use())
        .map_err(refused)?
        .into_iter()
        .filter(|f| f.sha256 == sha256)
        .collect();
    let mut last = FileError::Missing;
    for file in uses {
        match fetch_somewhere(store, remote, blobs, &file) {
            Ok(()) => return Ok(()),
            Err(FileError::Remote(e)) => return Err(FileError::Remote(e)),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// A files round: uploads, fetches of files up to `fetch_up_to` bytes
/// (thumbnails whatever their size), releases. Nothing happens with a
/// server that keeps no files.
///
/// # Errors
///
/// The server cannot be reached or signed the device out, the vault fails.
/// A file the server refuses is noted and the round goes on.
pub fn transfer(
    store: &impl Access,
    blobs: &BlobStore,
    remote: &mut impl Remote,
    fetch_up_to: u64,
) -> Result<FilesRound, SyncError> {
    let mut round = FilesRound::default();
    let Some(info) = remote.files_info()? else {
        return Ok(round);
    };
    store.with(crate::store::Store::assign_file_epochs)?;
    let mut full: Vec<String> = Vec::new();
    for file in store.with(|s| s.files_to_upload())? {
        if full.contains(&file.space) || !blobs.has(&file.sha256)? {
            continue;
        }
        let keying = store.with(|s| s.file_keying(&file.space, file.epoch))?;
        let Some(address) = address(&keying, &file.sha256) else {
            continue;
        };
        match upload(remote, blobs, &file, &keying, &address, info.part_bytes) {
            Ok(sent) => {
                store.with(|s| s.file_on_server(&file.space, &file.sha256, &address))?;
                round.uploaded += u32::from(sent);
            }
            Err(FileError::Remote(e)) => return Err(e.into()),
            // another device sends it at the same time, or an answer was
            // lost: where it stands is asked again next round
            Err(FileError::Refused(code)) if code == error_code::UPLOAD_MOVED => {}
            Err(FileError::Refused(code)) if code == error_code::QUOTA_FULL => {
                round.quota_full = true;
                full.push(file.space.clone());
            }
            Err(FileError::Refused(code)) => {
                store.with(|s| s.file_refused(&file.space, &file.sha256, &code))?;
                round.refused += 1;
            }
            // another device's upload was let go meanwhile: next round
            Err(FileError::Missing) => {}
        }
    }
    for file in store.with(|s| s.files_in_use())? {
        let wanted = file
            .size
            .is_none_or(|size| u64::try_from(size).unwrap_or(u64::MAX) <= fetch_up_to);
        if !wanted || blobs.has(&file.sha256)? {
            continue;
        }
        match fetch_somewhere(store, remote, blobs, &file) {
            Ok(()) => round.fetched += 1,
            Err(FileError::Remote(e)) => return Err(e.into()),
            // not uploaded yet, or damaged on the way: next round
            Err(FileError::Missing | FileError::Refused(_)) => {}
        }
    }
    for gone in store.with(|s| s.files_to_release())? {
        if store.with(|s| s.sync_pending(&gone.space))? {
            continue;
        }
        let Some(address) = gone.address.clone() else {
            store.with(|s| s.file_released(&gone.space, &gone.sha256))?;
            continue;
        };
        let (head, _) = store.with(|s| s.sync_cursor(&gone.space))?;
        match remote.release_file(space_of(&gone.space), &address, head) {
            Ok(true) | Err(FileError::Missing) => {
                store.with(|s| s.file_released(&gone.space, &gone.sha256))?;
                round.released += 1;
            }
            // the space moved on, or another refusal: the next round sees
            Ok(false) | Err(FileError::Refused(_)) => {}
            Err(FileError::Remote(e)) => return Err(e.into()),
        }
    }
    Ok(round)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealing_and_opening_streams_match_the_whole_file() {
        let files = app_e2ee::files_key_from(&[4u8; 32]);
        for len in [0, 1, CHUNK - 1, CHUNK, CHUNK + 1, 3 * CHUNK] {
            let plain: Vec<u8> = (0..len).map(|i| u8::try_from(i % 239).unwrap()).collect();
            let sha = crate::blobs::sha256_hex(&plain);
            let mut sealed = Vec::new();
            Sealing::new(&plain[..], &files, &sha)
                .read_to_end(&mut sealed)
                .unwrap();
            assert_eq!(
                sealed,
                app_e2ee::seal_file(&files, &sha, &plain).unwrap(),
                "{len}"
            );
            let mut back = Vec::new();
            Opening::new(&sealed[..], files.clone(), &sha)
                .read_to_end(&mut back)
                .unwrap();
            assert_eq!(back, plain, "{len}");
            if len > 0 {
                let cut = &sealed[..sealed.len() - 1];
                let mut out = Vec::new();
                assert!(
                    Opening::new(cut, files.clone(), &sha)
                        .read_to_end(&mut out)
                        .is_err()
                );
            }
        }
    }
}
