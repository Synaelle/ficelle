// SPDX-License-Identifier: AGPL-3.0-or-later

//! Attachment files with the server, after the notes' round: upload what
//! this device attached, fetch what its notes use and it lacks, let go of
//! what nothing uses any more. A file of an e2ee account is sealed here
//! (see [`app_e2ee::seal_file`]), a standard one goes as it is and the
//! server seals it.
//!
//! Uploads resume where the server says they stand: sealing is
//! deterministic, so the bytes past that point are the same. Downloads are
//! checked against the file's hash before they are kept. A file is only let
//! go when everything here is on the server, and the server refuses if the
//! space moved on meanwhile (another device may use the file).
//!
//! A [`Progress`] hears how far the files of a pass or a fetch got, after
//! each part sent and every few hundred kilobytes received.

use std::io::Read;

use app_e2ee::{CHUNK, FILE_HEADER, FilesKey, Opener, SEALED_CHUNK, Sealer};
use app_protocol::error_code;
use app_protocol::files::{FileState, FileStatus};
use serde::Serialize;

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

/// Bytes received between two reports of a download.
const REPORT_EVERY: u64 = 256 * 1024;

/// A file on its way to or from the server, for the progress the apps show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Moving {
    /// Hex SHA-256 of the file.
    pub sha256: String,
    /// A name it is attached under.
    pub name: String,
    /// Sent to the server, else received from it.
    pub sending: bool,
    /// Bytes of the file moved so far.
    pub done: u64,
    /// The file's size, in bytes.
    pub total: u64,
}

/// Hears where the files of a pass or a fetch stand.
pub trait Progress {
    /// The files still to move, the one moving first: as it starts, as it
    /// goes and once it is through, sent or not. Empty when none is left.
    /// A pass or fetch that fails may stop without the last word: what was
    /// heard is over once the call returns.
    fn files(&mut self, moving: &[Moving]);
}

/// No one to tell.
impl Progress for () {
    fn files(&mut self, _: &[Moving]) {}
}

/// The files of a pass or a fetch, told to a [`Progress`]. Files of unknown
/// size (thumbnails) are not in it: they are small, and nothing could say
/// how far they are.
struct Queue<'a> {
    files: Vec<Moving>,
    progress: &'a mut dyn Progress,
}

impl Queue<'_> {
    fn add(&mut self, file: &FileUse, sending: bool, size: Option<u64>) {
        if let Some(total) = size {
            self.files.push(Moving {
                sha256: file.sha256.clone(),
                name: file.name.clone(),
                sending,
                done: 0,
                total,
            });
        }
    }

    fn moved(&mut self, sha256: &str, done: u64) {
        let Some(at) = self.files.iter().position(|f| f.sha256 == sha256) else {
            return;
        };
        self.files[..=at].rotate_right(1);
        self.files[0].done = done.min(self.files[0].total);
        self.progress.files(&self.files);
    }

    fn finished(&mut self, sha256: &str) {
        let before = self.files.len();
        self.files.retain(|f| f.sha256 != sha256);
        if self.files.len() != before {
            self.progress.files(&self.files);
        }
    }

    /// The files left go another time.
    fn clear(&mut self) {
        if !self.files.is_empty() {
            self.files.clear();
            self.progress.files(&self.files);
        }
    }
}

/// Reads a download, telling how far it got.
struct Counted<'a, R> {
    source: R,
    done: u64,
    told: u64,
    moved: &'a mut dyn FnMut(u64),
}

impl<R: Read> Read for Counted<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.source.read(buf)?;
        self.done += n as u64;
        if n == 0 || self.done - self.told >= REPORT_EVERY {
            self.told = self.done;
            (self.moved)(self.done);
        }
        Ok(n)
    }
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
    /// The account has no room left.
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

/// Where a file lives on the server.
fn address(keying: &FileKeying, sha256: &str) -> String {
    match keying {
        FileKeying::Plain => sha256.to_owned(),
        FileKeying::Sealed(files) => app_e2ee::file_address(files, sha256),
    }
}

/// Sends one file, from where the server says it stands. Returns whether
/// any bytes went: a file the server holds, or takes back, sends none.
fn upload(
    remote: &mut impl Remote,
    blobs: &BlobStore,
    file: &FileUse,
    keying: &FileKeying,
    address: &str,
    part_bytes: u64,
    moved: &mut dyn FnMut(u64),
) -> Result<bool, FileError> {
    let fail = |e: &dyn std::fmt::Display| FileError::Refused(format!("local: {e}"));
    let Some(plain) = blobs.size(&file.sha256).map_err(|e| fail(&e))? else {
        return Ok(false);
    };
    let total = match keying {
        FileKeying::Sealed(_) => app_e2ee::sealed_size(plain),
        FileKeying::Plain => plain,
    };
    let state: FileState = remote.file_state(address)?;
    let mut start = match state.status {
        FileStatus::Complete => return Ok(false),
        FileStatus::Kept => return remote.keep_file(address).map(|()| false),
        FileStatus::Partial if state.size == total => state.received,
        _ => 0,
    };
    // told in the plain file's bytes, the size the person knows
    let plain_of = |sent: u64| {
        u64::try_from(u128::from(sent) * u128::from(plain) / u128::from(total.max(1)))
            .unwrap_or(plain)
    };
    moved(plain_of(start));
    let source = blobs
        .open(&file.sha256)
        .map_err(|e| fail(&e))?
        .ok_or(FileError::Missing)?;
    let mut reader: Box<dyn Read> = match keying {
        FileKeying::Sealed(files) => Box::new(Sealing::new(source, files, &file.sha256)),
        FileKeying::Plain => Box::new(source),
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
            address,
            FilePart {
                start,
                total,
                bytes: &bytes,
                declared: declared.filter(|_| start == 0),
            },
        )?;
        start += bytes.len() as u64;
        moved(plain_of(start));
        if answer.status == FileStatus::Complete || start >= total {
            return Ok(true);
        }
    }
}

/// Fetches one file into the vault. Not marked as held: the next upload
/// pass asks the server, which takes back a file let go meanwhile.
fn fetch_one(
    store: &impl Access,
    remote: &mut impl Remote,
    blobs: &BlobStore,
    sha256: &str,
    moved: &mut dyn FnMut(u64),
) -> Result<(), FileError> {
    let keying = store
        .with(|s| s.file_keying())
        .map_err(|e| FileError::Refused(format!("local: {e}")))?;
    let source = remote.get_file(&address(&keying, sha256))?;
    let reader: Box<dyn Read> = match keying {
        FileKeying::Sealed(files) => Box::new(Opening::new(source, files, sha256)),
        FileKeying::Plain => source,
    };
    let mut reader = Counted {
        source: reader,
        done: 0,
        told: 0,
        moved,
    };
    blobs
        .put_known(sha256, &mut reader)
        .map_err(|e| FileError::Refused(format!("local: {e}")))
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
    progress: &mut dyn Progress,
) -> Result<(), FileError> {
    if blobs.has(sha256).unwrap_or(false) {
        return Ok(());
    }
    let refused = |e: Error| FileError::Refused(format!("local: {e}"));
    let used = store
        .with(|s| s.files_in_use())
        .map_err(refused)?
        .into_iter()
        .find(|f| f.sha256 == sha256)
        .ok_or(FileError::Missing)?;
    let mut queue = Queue {
        files: Vec::new(),
        progress,
    };
    queue.add(&used, false, size_of(&used));
    queue.progress.files(&queue.files);
    let done = fetch_one(store, remote, blobs, sha256, &mut |n| {
        queue.moved(sha256, n);
    });
    queue.finished(sha256);
    done
}

/// A file's size as its note says, `None` for a thumbnail.
fn size_of(file: &FileUse) -> Option<u64> {
    file.size.and_then(|size| u64::try_from(size).ok())
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
    progress: &mut dyn Progress,
) -> Result<FilesRound, SyncError> {
    let mut round = FilesRound::default();
    let Some(info) = remote.files_info()? else {
        return Ok(round);
    };
    let keying = store.with(|s| s.file_keying())?;
    let mut queue = Queue {
        files: Vec::new(),
        progress,
    };
    let mut uploads = Vec::new();
    for file in store.with(|s| s.files_to_upload())? {
        if let Some(size) = blobs.size(&file.sha256)? {
            queue.add(&file, true, Some(size));
            uploads.push(file);
        }
    }
    for file in uploads {
        if round.quota_full {
            queue.clear();
            break;
        }
        let address = address(&keying, &file.sha256);
        let sent = upload(
            remote,
            blobs,
            &file,
            &keying,
            &address,
            info.part_bytes,
            &mut |n| queue.moved(&file.sha256, n),
        );
        queue.finished(&file.sha256);
        match sent {
            Ok(sent) => {
                store.with(|s| s.file_on_server(&file.sha256, &address))?;
                round.uploaded += u32::from(sent);
            }
            Err(FileError::Remote(e)) => return Err(e.into()),
            // another device sends it at the same time, or an answer was
            // lost: where it stands is asked again next round
            Err(FileError::Refused(code)) if code == error_code::UPLOAD_MOVED => {}
            Err(FileError::Refused(code)) if code == error_code::QUOTA_FULL => {
                round.quota_full = true;
            }
            Err(FileError::Refused(code)) => {
                store.with(|s| s.file_refused(&file.sha256, &code))?;
                round.refused += 1;
            }
            // another device's upload was let go meanwhile: next round
            Err(FileError::Missing) => {}
        }
    }
    let mut missing = Vec::new();
    for file in store.with(|s| s.files_in_use())? {
        let wanted = file
            .size
            .is_none_or(|size| u64::try_from(size).unwrap_or(u64::MAX) <= fetch_up_to);
        if wanted && !blobs.has(&file.sha256)? {
            queue.add(&file, false, size_of(&file));
            missing.push(file);
        }
    }
    for file in missing {
        // fetched meanwhile, when a note asked for it
        if blobs.has(&file.sha256)? {
            queue.finished(&file.sha256);
            continue;
        }
        let fetched = fetch_one(store, remote, blobs, &file.sha256, &mut |n| {
            queue.moved(&file.sha256, n);
        });
        queue.finished(&file.sha256);
        match fetched {
            Ok(()) => round.fetched += 1,
            Err(FileError::Remote(e)) => return Err(e.into()),
            // not uploaded yet, or damaged on the way: next round
            Err(FileError::Missing | FileError::Refused(_)) => {}
        }
    }
    for gone in store.with(|s| s.files_to_release())? {
        if store.with(|s| s.sync_pending())? {
            continue;
        }
        let Some(address) = gone.address.clone() else {
            store.with(|s| s.file_released(&gone.sha256))?;
            continue;
        };
        let (head, _) = store.with(|s| s.sync_cursor())?;
        match remote.release_file(&address, head) {
            Ok(true) | Err(FileError::Missing) => {
                store.with(|s| s.file_released(&gone.sha256))?;
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
