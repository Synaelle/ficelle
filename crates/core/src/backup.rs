// SPDX-License-Identifier: AGPL-3.0-or-later

//! The backup archive: a zip holding `manifest.json` (what it is and what
//! it holds), `data.json` (folders, notes with ids, tags, dates, trash,
//! history, file descriptions) and `files/<sha256>` (the files' content).
//!
//! An encrypted backup is that zip sealed whole with the stream format of
//! the files (64 KB chunks, XChaCha20-Poly1305), under a key derived from a
//! password with Argon2id and a random salt kept in a header. The zip is
//! written as a stream, so it never needs to sit on the disk unsealed, and
//! read through a reader that opens chunks as they are asked for, so it
//! never does either.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use app_e2ee::{CHUNK, FILE_HEADER as SEAL_HEADER, FileCipher, SEALED_CHUNK, Sealer, plain_size};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::blobs::BlobStore;
use crate::crypto::KdfParams;
use crate::error::{Error, Result};
use crate::store::{BackupData, RestoreReport, Store};

/// The format this version writes, and the newest it reads.
pub const FORMAT: u32 = 1;

/// The start of an encrypted backup. A plain one starts like any zip.
const MAGIC: &[u8; 12] = b"NOTES BACKUP";
const SALT: usize = 16;
/// Magic, format, Argon2id memory, passes and lanes, salt.
const OUTER_HEADER: usize = MAGIC.len() + 4 + 4 + 4 + 4 + SALT;
/// What the sealing is bound to, as files are to their hash.
const LABEL: &str = "backup";
/// The most a backup's `data.json` may hold: folders, notes and history,
/// files apart.
const MAX_DATA: u64 = 1024 * 1024 * 1024;

/// What a backup is and what it holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    /// Format version.
    pub format: u32,
    /// The app that made it, and its version.
    pub app: String,
    /// Version of that app.
    pub app_version: String,
    /// Unix milliseconds.
    pub created_at: i64,
    /// Sealed with a password.
    pub encrypted: bool,
    /// Notes.
    pub notes: usize,
    /// Folders.
    pub folders: usize,
    /// Versions of notes.
    pub versions: usize,
    /// Files.
    pub files: usize,
    /// Files whose content this device did not have (never fetched): their
    /// descriptions are in, their content is not.
    pub files_missing: usize,
    /// Hashes of the files stored in it: one listed here that cannot be
    /// read means a damaged backup, not a file its device never had.
    #[serde(default)]
    pub stored: Vec<String>,
}

/// Seals what is written to it in chunks, the last one on `finish`.
struct SealingWriter<W: Write> {
    out: W,
    sealer: Sealer,
    buffer: Vec<u8>,
}

impl<W: Write> SealingWriter<W> {
    fn new(mut out: W, files: &Zeroizing<[u8; 32]>) -> Result<Self> {
        let sealer = Sealer::new(files, LABEL);
        out.write_all(&sealer.header())?;
        Ok(Self {
            out,
            sealer,
            buffer: Vec::with_capacity(CHUNK * 2),
        })
    }

    fn finish(mut self) -> Result<W> {
        let last = self
            .sealer
            .seal(&self.buffer, true)
            .map_err(|_| Error::Crypto)?;
        self.out.write_all(&last)?;
        self.out.flush()?;
        Ok(self.out)
    }
}

impl<W: Write> Write for SealingWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.buffer.extend_from_slice(bytes);
        // a full chunk is sealed once more follows: the last is sealed apart
        while self.buffer.len() > CHUNK {
            let sealed = self
                .sealer
                .seal(&self.buffer[..CHUNK], false)
                .map_err(|_| std::io::Error::other("sealing failed"))?;
            self.out.write_all(&sealed)?;
            self.buffer.drain(..CHUNK);
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.out.flush()
    }
}

/// Reads a sealed backup as its plain zip, opening chunks as asked for.
struct SealedReader {
    file: File,
    cipher: FileCipher,
    /// Where the first sealed chunk starts.
    start: u64,
    size: u64,
    chunks: u64,
    position: u64,
    current: Option<(u64, Zeroizing<Vec<u8>>)>,
}

impl SealedReader {
    fn new(mut file: File, offset: u64, files: &Zeroizing<[u8; 32]>) -> Result<Self> {
        let total = file.metadata()?.len();
        let size =
            plain_size(total - offset).ok_or_else(|| Error::Corrupt("backup size".into()))?;
        let cipher = FileCipher::new(files, LABEL);
        let mut header = [0u8; SEAL_HEADER];
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(&mut header)?;
        // the header comes from the key: another one means another password
        if header != cipher.header() {
            return Err(Error::WrongPassword);
        }
        Ok(Self {
            file,
            cipher,
            start: offset + SEAL_HEADER as u64,
            size,
            chunks: size.div_ceil(CHUNK as u64).max(1),
            position: 0,
            current: None,
        })
    }

    fn chunk(&mut self, index: u64) -> std::io::Result<&[u8]> {
        if self.current.as_ref().map(|(i, _)| *i) != Some(index) {
            let last = index + 1 == self.chunks;
            let length = if last {
                // under a chunk: the size and index say so
                usize::try_from(self.size - index * CHUNK as u64).unwrap_or(CHUNK)
                    + (SEALED_CHUNK - CHUNK)
            } else {
                SEALED_CHUNK
            };
            let mut sealed = vec![0u8; length];
            self.file
                .seek(SeekFrom::Start(self.start + index * SEALED_CHUNK as u64))?;
            self.file.read_exact(&mut sealed)?;
            let index32 =
                u32::try_from(index).map_err(|_| std::io::Error::other("backup too large"))?;
            let plain = self.cipher.open_at(index32, &sealed, last).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "backup damaged")
            })?;
            self.current = Some((index, plain));
        }
        Ok(self
            .current
            .as_ref()
            .map_or(&[][..], |(_, plain)| plain.as_slice()))
    }
}

impl Read for SealedReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.position >= self.size || out.is_empty() {
            return Ok(0);
        }
        let index = self.position / CHUNK as u64;
        let at = usize::try_from(self.position % CHUNK as u64).unwrap_or(0);
        let chunk = self.chunk(index)?;
        let n = (chunk.len() - at).min(out.len());
        out[..n].copy_from_slice(&chunk[at..at + n]);
        self.position += n as u64;
        Ok(n)
    }
}

impl Seek for SealedReader {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        let next = match to {
            SeekFrom::Start(p) => i128::from(p),
            SeekFrom::End(d) => i128::from(self.size) + i128::from(d),
            SeekFrom::Current(d) => i128::from(self.position) + i128::from(d),
        };
        self.position =
            u64::try_from(next).map_err(|_| std::io::Error::other("seek before start"))?;
        Ok(self.position)
    }
}

fn random_salt() -> Result<[u8; SALT]> {
    let mut salt = [0u8; SALT];
    getrandom::fill(&mut salt).map_err(|_| Error::Crypto)?;
    Ok(salt)
}

fn zip_error(e: zip::result::ZipError) -> Error {
    match e {
        zip::result::ZipError::Io(io) => Error::Io(io),
        other => Error::Corrupt(format!("backup: {other}")),
    }
}

/// The most a backup's header may ask of Argon2id: a file from elsewhere
/// must not make the device run out of memory or spin for minutes.
const MAX_KDF: KdfParams = KdfParams {
    memory_kib: 1024 * 1024,
    iterations: 32,
    parallelism: 16,
};

/// Writes a backup of the vault to `path`, encrypted when a password is
/// given. Written beside it first and moved in place at the end, so a
/// failed backup never leaves half a file under the name.
///
/// # Errors
///
/// Database, file and crypto errors.
pub fn write_backup(
    store: &Store,
    blobs: &BlobStore,
    path: &Path,
    password: Option<&str>,
    kdf: KdfParams,
) -> Result<Manifest> {
    let partial = path.with_extension("partial");
    let written = File::create(&partial)
        .map_err(Error::from)
        .and_then(|file| write_backup_to(store, blobs, file, password, kdf));
    match written {
        Ok(manifest) => {
            std::fs::rename(&partial, path)?;
            Ok(manifest)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&partial);
            Err(e)
        }
    }
}

/// Writes a backup into a file already open (an Android document, which
/// has no path to write beside).
///
/// # Errors
///
/// As [`write_backup`].
pub fn write_backup_to(
    store: &Store,
    blobs: &BlobStore,
    file: File,
    password: Option<&str>,
    kdf: KdfParams,
) -> Result<Manifest> {
    let data = store.backup_data()?;
    let hashes = data.file_hashes();
    let mut have = Vec::new();
    for sha in &hashes {
        if blobs.has(sha)? {
            have.push(sha.clone());
        }
    }
    have.sort();
    let manifest = Manifest {
        format: FORMAT,
        app: app_brand::DISPLAY_NAME.to_owned(),
        app_version: app_brand::VERSION.to_owned(),
        created_at: crate::clock::Clock::now(&crate::clock::SystemClock),
        encrypted: password.is_some(),
        notes: data.notes.len(),
        folders: data.folders.len(),
        versions: data.versions.len(),
        files: have.len(),
        files_missing: hashes.len() - have.len(),
        stored: have.clone(),
    };

    let mut out = BufWriter::new(file);
    match password {
        Some(password) => {
            let salt = random_salt()?;
            out.write_all(MAGIC)?;
            out.write_all(&FORMAT.to_le_bytes())?;
            out.write_all(&kdf.memory_kib.to_le_bytes())?;
            out.write_all(&kdf.iterations.to_le_bytes())?;
            out.write_all(&kdf.parallelism.to_le_bytes())?;
            out.write_all(&salt)?;
            let key = kdf.derive_key(password, &salt)?;
            let sealing = SealingWriter::new(out, &app_e2ee::files_key_from(&key))?;
            let sealing = write_zip(sealing, &manifest, &data, &have, blobs)?;
            sealing
                .finish()?
                .into_inner()
                .map_err(|e| Error::Io(e.into_error()))?;
        }
        None => {
            write_zip(out, &manifest, &data, &have, blobs)?
                .into_inner()
                .map_err(|e| Error::Io(e.into_error()))?;
        }
    }
    Ok(manifest)
}

fn write_zip<W: Write>(
    out: W,
    manifest: &Manifest,
    data: &BackupData,
    files: &[String],
    blobs: &BlobStore,
) -> Result<W> {
    let mut zip = zip::ZipWriter::new_stream(out);
    let text = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(true);
    // pictures and documents are compressed already
    let stored = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .large_file(true);
    zip.start_file("manifest.json", text).map_err(zip_error)?;
    serde_json::to_writer_pretty(&mut zip, manifest).map_err(|e| Error::Invalid(e.to_string()))?;
    zip.start_file("data.json", text).map_err(zip_error)?;
    serde_json::to_writer(&mut zip, data).map_err(|e| Error::Invalid(e.to_string()))?;
    for sha in files {
        let Some(mut reader) = blobs.open(sha)? else {
            continue;
        };
        zip.start_file(format!("files/{sha}"), stored)
            .map_err(zip_error)?;
        std::io::copy(&mut reader, &mut zip)?;
    }
    Ok(zip.finish().map_err(zip_error)?.into_inner())
}

trait ReadSeek: Read + Seek + Send {}
impl<T: Read + Seek + Send> ReadSeek for T {}

/// A backup opened for reading.
pub struct Backup {
    archive: zip::ZipArchive<Box<dyn ReadSeek>>,
    manifest: Manifest,
}

impl std::fmt::Debug for Backup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Backup")
            .field("manifest", &self.manifest)
            .finish_non_exhaustive()
    }
}

/// Whether a file is an encrypted backup (it then needs a password).
///
/// # Errors
///
/// The file cannot be read.
pub fn is_encrypted(path: &Path) -> Result<bool> {
    is_encrypted_file(&mut File::open(path)?)
}

/// As [`is_encrypted`], for a file already open, read from its start.
///
/// # Errors
///
/// The file cannot be read.
pub fn is_encrypted_file(file: &mut File) -> Result<bool> {
    let mut head = [0u8; MAGIC.len()];
    file.seek(SeekFrom::Start(0))?;
    let sealed = file.read_exact(&mut head).is_ok() && &head == MAGIC;
    file.seek(SeekFrom::Start(0))?;
    Ok(sealed)
}

impl Backup {
    /// Opens a backup, with its password when it is encrypted.
    ///
    /// # Errors
    ///
    /// [`Error::WrongPassword`] for a wrong or missing password,
    /// [`Error::Corrupt`] for a damaged backup or a newer format.
    pub fn open(path: &Path, password: Option<&str>) -> Result<Self> {
        Self::open_file(File::open(path)?, password)
    }

    /// Opens a backup from a file already open (an Android document).
    ///
    /// # Errors
    ///
    /// As [`Backup::open`].
    pub fn open_file(mut file: File, password: Option<&str>) -> Result<Self> {
        let reader: Box<dyn ReadSeek> = if is_encrypted_file(&mut file)? {
            let password = password.ok_or(Error::WrongPassword)?;
            let mut header = [0u8; OUTER_HEADER];
            file.read_exact(&mut header)
                .map_err(|_| Error::Corrupt("backup header".into()))?;
            let word = |at: usize| {
                u32::from_le_bytes([header[at], header[at + 1], header[at + 2], header[at + 3]])
            };
            let at = MAGIC.len();
            if word(at) > FORMAT {
                return Err(Error::Corrupt(
                    "this backup was made by a newer version".into(),
                ));
            }
            let kdf = KdfParams {
                memory_kib: word(at + 4),
                iterations: word(at + 8),
                parallelism: word(at + 12),
            };
            if kdf.memory_kib > MAX_KDF.memory_kib
                || kdf.iterations > MAX_KDF.iterations
                || kdf.parallelism > MAX_KDF.parallelism
            {
                return Err(Error::Corrupt("backup key settings out of range".into()));
            }
            let key = kdf.derive_key(password, &header[at + 16..])?;
            Box::new(SealedReader::new(
                file,
                OUTER_HEADER as u64,
                &app_e2ee::files_key_from(&key),
            )?)
        } else {
            Box::new(BufReader::new(file))
        };
        let mut archive = zip::ZipArchive::new(reader).map_err(zip_error)?;
        let manifest: Manifest =
            serde_json::from_reader(archive.by_name("manifest.json").map_err(zip_error)?)
                .map_err(|e| Error::Corrupt(format!("backup manifest: {e}")))?;
        if manifest.format > FORMAT {
            return Err(Error::Corrupt(
                "this backup was made by a newer version".into(),
            ));
        }
        Ok(Self { archive, manifest })
    }

    /// What the backup is and holds.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Folders, notes, history and file descriptions.
    ///
    /// # Errors
    ///
    /// A damaged backup.
    pub fn data(&mut self) -> Result<BackupData> {
        // read within a bound: a small backup cannot claim gigabytes of
        // notes
        serde_json::from_reader(BufReader::new(
            self.archive
                .by_name("data.json")
                .map_err(zip_error)?
                .take(MAX_DATA),
        ))
        .map_err(|e| Error::Corrupt(format!("backup data: {e}")))
    }

    /// Puts the backup back into the vault: the files of the notes that
    /// come back first (checked against their hash), then the folders and
    /// notes with their ids. Notes already here are left as they are.
    ///
    /// # Errors
    ///
    /// A damaged backup, database and file errors.
    pub fn restore(&mut self, store: &mut Store, blobs: &BlobStore) -> Result<RestoreReport> {
        let data = self.data()?;
        let present = store.notes_present(&data.notes.iter().map(|n| n.id).collect::<Vec<_>>())?;
        let needed: std::collections::HashSet<String> = data
            .attachments
            .iter()
            .filter(|a| !present.contains(&a.note_id))
            .flat_map(|a| std::iter::once(a.sha256.clone()).chain(a.thumb_sha256.clone()))
            .collect();
        for sha in &needed {
            if blobs.has(sha)? {
                continue;
            }
            let entry = format!("files/{sha}");
            match self.archive.by_name(&entry) {
                Ok(mut file) => blobs.put_known(sha, &mut file)?,
                Err(zip::result::ZipError::FileNotFound) if !self.manifest.stored.contains(sha) => {
                }
                Err(zip::result::ZipError::FileNotFound) => {
                    return Err(Error::Corrupt(format!("backup: {entry} is missing")));
                }
                Err(e) => return Err(zip_error(e)),
            }
        }
        let has = |sha: &str| blobs.has(sha).unwrap_or(false);
        store.restore_backup(&data, &has)
    }
}

/// The name offered for a new backup: the app and the day.
pub fn default_name(now: i64) -> PathBuf {
    let date = jiff::Timestamp::from_millisecond(now)
        .map(|t| t.strftime("%Y-%m-%d").to_string())
        .unwrap_or_default();
    PathBuf::from(format!(
        "{} {date}.{}",
        app_brand::DISPLAY_NAME,
        app_brand::BACKUP_EXTENSION
    ))
}

#[cfg(test)]
pub(crate) mod deep;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::test_support::store;
    use crate::store::{NewNote, NoteChange};

    const KDF: KdfParams = KdfParams::INSECURE_FOR_TESTS;

    /// A vault with a tree, tags, pin, trash, history and files, one of
    /// them several chunks long and one exactly a chunk.
    fn filled(dir: &Path) -> (Store, BlobStore) {
        let (mut store, clock) = store();
        let blobs = BlobStore::new(dir.join("blobs"), None);
        let top = store.create_folder(None, "Projects").unwrap().id;
        let inner = store.create_folder(Some(top), "Garden").unwrap().id;
        let note = store
            .create_note(&NewNote {
                folder_id: Some(inner),
                title: "Plan".into(),
                body: "first [[Other]]".into(),
                tags: vec!["home".into(), "work".into()],
                pinned: true,
            })
            .unwrap();
        clock.advance(crate::store::EDIT_SESSION + 1);
        store
            .update_note(
                note.id,
                NoteChange {
                    body: Some("second [[Other]]".into()),
                    ..NoteChange::default()
                },
            )
            .unwrap();
        let other = store
            .create_note(&NewNote {
                title: "Other".into(),
                body: "x".into(),
                ..NewNote::default()
            })
            .unwrap();
        let gone = store
            .create_note(&NewNote {
                title: "Gone".into(),
                ..NewNote::default()
            })
            .unwrap();
        store.trash_note(gone.id).unwrap();
        let big: Vec<u8> = (0..300_000u32).map(|i| (i % 253) as u8).collect();
        let exact = vec![5u8; CHUNK];
        for (name, bytes) in [("big.bin", &big[..]), ("exact.bin", &exact[..])] {
            let sha = blobs.put(bytes).unwrap();
            store
                .add_attachment(
                    note.id,
                    name,
                    "application/octet-stream",
                    i64::try_from(bytes.len()).unwrap(),
                    &sha,
                )
                .unwrap();
        }
        let pic = blobs.put(b"picture").unwrap();
        let thumb = blobs.put(b"small").unwrap();
        let a = store
            .add_attachment(other.id, "p.png", "image/png", 7, &pic)
            .unwrap();
        store.set_attachment_thumb(a.id, &thumb).unwrap();
        (store, blobs)
    }

    fn same(a: &BackupData, b: &BackupData) {
        assert_eq!(a.folders, b.folders);
        assert_eq!(a.notes, b.notes);
        assert_eq!(a.versions, b.versions);
        assert_eq!(a.attachments, b.attachments);
    }

    #[test]
    fn a_plain_backup_restores_the_vault_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        let (source, blobs) = filled(dir.path());
        let path = dir.path().join("vault.backup");
        let manifest = write_backup(&source, &blobs, &path, None, KDF).unwrap();
        assert_eq!(
            (manifest.notes, manifest.folders, manifest.files),
            (3, 2, 4)
        );
        assert!(manifest.versions >= 1);
        assert!(!manifest.encrypted && !is_encrypted(&path).unwrap());
        assert!(!dir.path().join("vault.partial").exists());
        // any unzip tool reads it
        let plain = zip::ZipArchive::new(File::open(&path).unwrap()).unwrap();
        assert_eq!(plain.len(), 6);

        let (mut target, _) = store();
        let target_blobs = BlobStore::new(dir.path().join("restored"), None);
        let mut backup = Backup::open(&path, None).unwrap();
        assert_eq!(backup.manifest(), &manifest);
        let report = backup.restore(&mut target, &target_blobs).unwrap();
        assert_eq!(
            (report.notes, report.folders, report.files, report.skipped),
            (3, 2, 3, 0)
        );
        same(
            &source.backup_data().unwrap(),
            &target.backup_data().unwrap(),
        );
        for a in &source.backup_data().unwrap().attachments {
            assert_eq!(
                target_blobs.get(&a.sha256).unwrap(),
                blobs.get(&a.sha256).unwrap()
            );
        }
        // links come back with their notes
        let other = target
            .backup_data()
            .unwrap()
            .notes
            .into_iter()
            .find(|n| n.title == "Other")
            .unwrap();
        assert_eq!(target.backlinks(other.id).unwrap().len(), 1);

        // a second restore changes nothing
        let again = Backup::open(&path, None)
            .unwrap()
            .restore(&mut target, &target_blobs)
            .unwrap();
        assert_eq!((again.notes, again.folders, again.skipped), (0, 0, 3));
    }

    #[test]
    fn an_encrypted_backup_needs_its_password_and_hides_everything() {
        let dir = tempfile::tempdir().unwrap();
        let (source, blobs) = filled(dir.path());
        let path = dir.path().join("sealed.backup");
        write_backup(&source, &blobs, &path, Some("correct horse"), KDF).unwrap();
        assert!(is_encrypted(&path).unwrap());
        let bytes = std::fs::read(&path).unwrap();
        for needle in [&b"second"[..], b"Projects", b"manifest.json", b"PK\x03\x04"] {
            assert!(
                !bytes.windows(needle.len()).any(|w| w == needle),
                "{needle:?} visible"
            );
        }
        assert!(matches!(
            Backup::open(&path, Some("wrong")),
            Err(Error::WrongPassword)
        ));
        assert!(matches!(
            Backup::open(&path, None),
            Err(Error::WrongPassword)
        ));

        let (mut target, _) = store();
        let target_blobs = BlobStore::new(dir.path().join("restored"), None);
        let mut backup = Backup::open(&path, Some("correct horse")).unwrap();
        assert!(backup.manifest().encrypted);
        backup.restore(&mut target, &target_blobs).unwrap();
        same(
            &source.backup_data().unwrap(),
            &target.backup_data().unwrap(),
        );

        // one byte changed in the middle: refused, not read wrong
        let mut damaged = bytes.clone();
        let middle = damaged.len() / 2;
        damaged[middle] ^= 1;
        let bad = dir.path().join("damaged.backup");
        std::fs::write(&bad, damaged).unwrap();
        let restored = Backup::open(&bad, Some("correct horse")).and_then(|mut b| {
            let (mut t, _) = store();
            b.restore(&mut t, &BlobStore::new(dir.path().join("bad"), None))
        });
        assert!(restored.is_err());
    }

    #[test]
    fn notes_already_here_stay_and_missing_files_are_counted() {
        let dir = tempfile::tempdir().unwrap();
        let (mut source, blobs) = filled(dir.path());
        let plan = source
            .backup_data()
            .unwrap()
            .notes
            .into_iter()
            .find(|n| n.title == "Plan")
            .unwrap();
        // a file this device never had
        let lost = "ab".repeat(32);
        source
            .add_attachment(plan.id, "lost.pdf", "application/pdf", 3, &lost)
            .unwrap();
        let path = dir.path().join("b.backup");
        let manifest = write_backup(&source, &blobs, &path, None, KDF).unwrap();
        assert_eq!(manifest.files_missing, 1);

        // restoring into the same vault after an edit there keeps the edit
        source
            .update_note(
                plan.id,
                NoteChange {
                    title: Some("Plan, edited".into()),
                    ..NoteChange::default()
                },
            )
            .unwrap();
        let report = Backup::open(&path, None)
            .unwrap()
            .restore(&mut source, &blobs)
            .unwrap();
        assert_eq!((report.notes, report.skipped), (0, 3));
        assert_eq!(source.note(plan.id).unwrap().title, "Plan, edited");

        // elsewhere: the note comes without the file it never had
        let (mut target, _) = store();
        let report = Backup::open(&path, None)
            .unwrap()
            .restore(&mut target, &BlobStore::new(dir.path().join("t"), None))
            .unwrap();
        assert_eq!(report.files_missing, 1);
        assert!(
            target
                .list_attachments(plan.id)
                .unwrap()
                .iter()
                .all(|a| a.file_name != "lost.pdf")
        );
    }

    #[test]
    fn a_folder_restored_under_a_parent_the_vault_still_has() {
        let dir = tempfile::tempdir().unwrap();
        let (mut source, blobs) = filled(dir.path());
        let path = dir.path().join("c.backup");
        write_backup(&source, &blobs, &path, None, KDF).unwrap();
        // the inner folder and its note are deleted for good, the top stays
        let data = source.backup_data().unwrap();
        let garden = data.folders.iter().find(|f| f.name == "Garden").unwrap();
        source.trash_folder(garden.id).unwrap();
        source.purge_folder(garden.id).unwrap();
        let report = Backup::open(&path, None)
            .unwrap()
            .restore(&mut source, &blobs)
            .unwrap();
        assert_eq!((report.folders, report.notes), (1, 1));
        let back = source.folder(garden.id).unwrap();
        assert_eq!(back.parent_id, garden.parent_id);
        assert_eq!(back.trashed_at, None);
    }
}
