// SPDX-License-Identifier: AGPL-3.0-or-later

//! What an import reads from: a folder, a zip (with zips inside it, as
//! Notion makes them for large exports), a tar (Joplin's JEX) or one file.
//! Each is a list of entries, read when needed, one at a time: an export of
//! several gigabytes never sits in memory whole.
//!
//! Entry paths use `/`, relative to the source. An entry of a zip inside a
//! zip is named `outer/inner.zip/path`.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// The largest entry read whole, for the page to parse. Larger ones (videos
/// and the like) are only copied as attachments, a piece at a time.
pub const MAX_READ: u64 = 256 * 1024 * 1024;

/// The largest file taken from an export as a note's file.
pub const MAX_FILE: u64 = 4 * 1024 * 1024 * 1024;

/// One file of a source.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// Path inside the source, with `/`.
    pub path: String,
    /// Size in bytes.
    pub size: u64,
    /// Last modified, unix milliseconds, when the source says.
    pub modified: Option<i64>,
}

/// Where an entry's bytes are.
#[derive(Debug, Clone)]
enum Place {
    /// A file on disk.
    File(PathBuf),
    /// The one file of a source opened from a handle (an Android document).
    Single,
    /// An entry of a zip, by index, in the zip at that position of `zips`.
    Zip { zip: usize, index: usize },
    /// Bytes of a tar on disk.
    Tar { offset: u64 },
}

/// An export opened for reading.
pub struct Source {
    entries: Vec<Entry>,
    places: Vec<Place>,
    /// Entries by path, to read any of thousands without a search.
    index: HashMap<String, usize>,
    zips: Vec<zip::ZipArchive<BufReader<File>>>,
    tar: Option<File>,
    single: Option<File>,
    /// Zips inside zips, written out to be read like the others, removed
    /// with the source.
    temp: Vec<PathBuf>,
    /// Bytes written out so far, for [`MAX_UNPACKED`].
    unpacked: u64,
    /// How deep the zip being read sits in other zips.
    nesting: u32,
}

/// The most an import writes out of gzip files and zips inside zips, and
/// how deep zips may sit in zips: a small file that unpacks without end
/// fills no disk.
pub const MAX_UNPACKED: u64 = 4 * 1024 * 1024 * 1024;
const MAX_NESTING: u32 = 3;

impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Source")
            .field("entries", &self.entries.len())
            .finish_non_exhaustive()
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        self.zips.clear();
        for path in &self.temp {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn bad(what: impl std::fmt::Display) -> Error {
    Error::Invalid(format!("this export cannot be read: {what}"))
}

fn millis(time: std::time::SystemTime) -> Option<i64> {
    time.duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
}

/// A zip's date (local time without a zone, as zips keep it) as unix
/// milliseconds, read as UTC: off by the zone at most, enough for notes
/// whose own dates are missing.
fn zip_millis(time: Option<zip::DateTime>) -> Option<i64> {
    let t = time?;
    let date = jiff::civil::date(
        i16::try_from(t.year()).ok()?,
        i8::try_from(t.month()).ok()?,
        i8::try_from(t.day()).ok()?,
    );
    let at = date
        .at(
            i8::try_from(t.hour()).ok()?,
            i8::try_from(t.minute()).ok()?,
            i8::try_from(t.second()).ok()?,
            0,
        )
        .to_zoned(jiff::tz::TimeZone::UTC)
        .ok()?;
    Some(at.timestamp().as_millisecond())
}

/// A path from an archive made safe: `/` separators, no leading `/`, no
/// `..` or `.` parts.
fn clean(path: &str) -> Option<String> {
    let parts: Vec<&str> = path
        .split(['/', '\\'])
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    if parts.contains(&"..") || parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
}

impl Source {
    /// Opens a folder, an archive, or a single file.
    ///
    /// # Errors
    ///
    /// The path cannot be read, or the archive is damaged.
    pub fn open(path: &Path) -> Result<Self> {
        let mut source = Self::empty();
        if path.is_dir() {
            source.walk(path, "")?;
        } else {
            let name = path
                .file_name()
                .map_or_else(|| "file".to_owned(), |n| n.to_string_lossy().into_owned());
            source.add_file(File::open(path)?, &name)?;
        }
        source.index_entries();
        Ok(source)
    }

    /// Opens a file already open, named `name`: what Android hands over
    /// for a document the person picked, which has no path.
    ///
    /// # Errors
    ///
    /// As [`Source::open`].
    pub fn open_file(mut file: File, name: &str) -> Result<Self> {
        let mut source = Self::empty();
        let name = if name.contains('.') {
            name.to_owned()
        } else {
            named_by_content(&mut file, name)?
        };
        source.add_file(file, &name)?;
        source.index_entries();
        Ok(source)
    }

    fn empty() -> Self {
        Self {
            entries: Vec::new(),
            places: Vec::new(),
            index: HashMap::new(),
            zips: Vec::new(),
            tar: None,
            single: None,
            temp: Vec::new(),
            unpacked: 0,
            nesting: 0,
        }
    }

    fn index_entries(&mut self) {
        self.index = self
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.path.clone(), i))
            .collect();
    }

    /// A file by what it starts with: a zip, a tar, gzip (Google Takeout's
    /// `.tgz`, written out whole and looked at again) or one entry.
    fn add_file(&mut self, mut file: File, name: &str) -> Result<()> {
        let mut head = [0u8; 512];
        let n = read_up_to(&mut file, &mut head)?;
        file.seek(SeekFrom::Start(0))?;
        if head[..n].starts_with(b"PK\x03\x04") || head[..n].starts_with(b"PK\x05\x06") {
            self.add_zip(file, "")
        } else if n == 512 && &head[257..262] == b"ustar" {
            self.add_tar(file)
        } else if head[..n].starts_with(&[0x1f, 0x8b]) {
            let temp =
                std::env::temp_dir().join(format!("import-{}.out", crate::id::NoteId::new()));
            self.temp.push(temp.clone());
            {
                let gz = flate2::read::GzDecoder::new(BufReader::new(file));
                self.unpack(gz, &temp)?;
            }
            let inner = name
                .strip_suffix(".gz")
                .or_else(|| name.strip_suffix(".tgz"))
                .unwrap_or(name);
            self.add_file(File::open(&temp)?, inner)
        } else {
            let meta = file.metadata()?;
            self.entries.push(Entry {
                path: name.to_owned(),
                size: meta.len(),
                modified: meta.modified().ok().and_then(millis),
            });
            self.places.push(Place::Single);
            self.single = Some(file);
            Ok(())
        }
    }

    /// Every entry, in the order found.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    fn walk(&mut self, dir: &Path, prefix: &str) -> Result<()> {
        let mut children: Vec<std::fs::DirEntry> = std::fs::read_dir(dir)?
            .filter_map(std::result::Result::ok)
            .collect();
        children.sort_by_key(std::fs::DirEntry::file_name);
        for child in children {
            let name = child.file_name().to_string_lossy().into_owned();
            let path = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let kind = child.file_type()?;
            // links are not followed: a link could lead out of the export
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                self.walk(&child.path(), &path)?;
            } else {
                let meta = child.metadata()?;
                self.entries.push(Entry {
                    path,
                    size: meta.len(),
                    modified: meta.modified().ok().and_then(millis),
                });
                self.places.push(Place::File(child.path()));
            }
        }
        Ok(())
    }

    fn add_zip(&mut self, file: File, prefix: &str) -> Result<()> {
        let mut archive = zip::ZipArchive::new(BufReader::new(file)).map_err(bad)?;
        let at = self.zips.len();
        let mut inner = Vec::new();
        for index in 0..archive.len() {
            let file = archive.by_index_raw(index).map_err(bad)?;
            if file.is_dir() {
                continue;
            }
            let Some(name) = file.name().ok().and_then(|n| clean(&n)) else {
                continue;
            };
            let name = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            if name.to_lowercase().ends_with(".zip") {
                inner.push((index, name));
                continue;
            }
            self.entries.push(Entry {
                path: name,
                size: file.size(),
                modified: zip_millis(file.last_modified()),
            });
            self.places.push(Place::Zip { zip: at, index });
        }
        self.zips.push(archive);
        // zips inside: written out next to the system's temporary files and
        // read the same way, their entries named after them
        if self.nesting >= MAX_NESTING {
            return Ok(());
        }
        for (index, name) in inner {
            let temp =
                std::env::temp_dir().join(format!("import-{}.zip", crate::id::NoteId::new()));
            self.temp.push(temp.clone());
            {
                let mut archive = std::mem::take(&mut self.zips);
                let file = archive[at].by_index(index).map_err(bad);
                let written = file.and_then(|file| self.unpack(file, &temp));
                self.zips = archive;
                written?;
            }
            self.nesting += 1;
            let added = self.add_zip(File::open(&temp)?, &name);
            self.nesting -= 1;
            added?;
        }
        Ok(())
    }

    /// Writes `from` out to `to`, within [`MAX_UNPACKED`] for the import.
    fn unpack(&mut self, from: impl Read, to: &std::path::Path) -> Result<()> {
        let room = MAX_UNPACKED.saturating_sub(self.unpacked);
        let mut out = File::create(to)?;
        let written = std::io::copy(&mut from.take(room + 1), &mut out).map_err(bad)?;
        if written > room {
            return Err(Error::Invalid(
                "this export unpacks to more than an import takes".into(),
            ));
        }
        self.unpacked += written;
        Ok(())
    }

    fn add_tar(&mut self, file: File) -> Result<()> {
        let mut archive = tar::Archive::new(file.try_clone()?);
        for entry in archive.entries().map_err(bad)? {
            let entry = entry.map_err(bad)?;
            if !entry.header().entry_type().is_file() {
                continue;
            }
            let Some(name) = entry.path().ok().and_then(|p| clean(&p.to_string_lossy())) else {
                continue;
            };
            let modified = entry
                .header()
                .mtime()
                .ok()
                .and_then(|s| i64::try_from(s).ok())
                .map(|s| s * 1000);
            self.entries.push(Entry {
                path: name,
                size: entry.size(),
                modified,
            });
            self.places.push(Place::Tar {
                offset: entry.raw_file_position(),
            });
        }
        self.tar = Some(file);
        Ok(())
    }

    fn place(&self, path: &str) -> Result<(usize, Place)> {
        self.index
            .get(path)
            .map(|&i| (i, self.places[i].clone()))
            .ok_or(Error::NotFound("entry"))
    }

    /// Opens an entry for reading a piece at a time.
    ///
    /// # Errors
    ///
    /// No such entry, a damaged archive, read errors.
    pub fn open_entry(&mut self, path: &str) -> Result<Box<dyn Read + '_>> {
        let (i, place) = self.place(path)?;
        let size = self.entries[i].size;
        Ok(match place {
            Place::File(file) => Box::new(BufReader::new(File::open(file)?)),
            Place::Zip { zip, index } => Box::new(self.zips[zip].by_index(index).map_err(bad)?),
            // a handle of the same file: one entry is read at a time
            Place::Tar { offset } => {
                let mut file = self
                    .tar
                    .as_ref()
                    .ok_or(Error::NotFound("tar"))?
                    .try_clone()?;
                file.seek(SeekFrom::Start(offset))?;
                Box::new(BufReader::new(file).take(size))
            }
            Place::Single => {
                let mut file = self
                    .single
                    .as_ref()
                    .ok_or(Error::NotFound("file"))?
                    .try_clone()?;
                file.seek(SeekFrom::Start(0))?;
                Box::new(BufReader::new(file))
            }
        })
    }

    /// Reads an entry whole.
    ///
    /// # Errors
    ///
    /// As [`Source::open_entry`], and entries over [`MAX_READ`].
    pub fn read(&mut self, path: &str) -> Result<Vec<u8>> {
        let (i, _) = self.place(path)?;
        let size = self.entries[i].size;
        if size > MAX_READ {
            return Err(Error::Invalid(format!("{path} is too large to read whole")));
        }
        let mut out = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
        // an archive can say less than its entry holds (a zip bomb): what
        // is read is capped, not what is said
        self.open_entry(path)?
            .take(MAX_READ + 1)
            .read_to_end(&mut out)?;
        if out.len() as u64 > MAX_READ {
            return Err(Error::Invalid(format!("{path} is too large to read whole")));
        }
        Ok(out)
    }
}

impl Source {
    /// Puts an entry in the vault's files, read twice a piece at a time (the
    /// hash first, the sealing needs it) rather than held whole. Returns its
    /// hash and size.
    ///
    /// # Errors
    ///
    /// As [`Source::open_entry`], file and crypto errors.
    pub fn store(&mut self, path: &str, blobs: &crate::blobs::BlobStore) -> Result<(String, u64)> {
        // the hash pass is capped too: a bomb is stopped before any write
        let (sha256, size) =
            crate::blobs::sha256_of_reader(&mut self.open_entry(path)?.take(MAX_FILE + 1))?;
        if size > MAX_FILE {
            return Err(Error::Invalid(format!("{path} is too large")));
        }
        blobs.put_known(&sha256, &mut self.open_entry(path)?)?;
        Ok((sha256, size))
    }
}

/// A name for a file that came without one (Android documents often have
/// only a number): the formats known by their extension get it from what
/// the file starts with.
fn named_by_content(file: &mut File, name: &str) -> Result<String> {
    let mut head = [0u8; 4096];
    let n = read_up_to(file, &mut head)?;
    file.seek(SeekFrom::Start(0))?;
    let text = String::from_utf8_lossy(&head[..n]);
    let start = text.trim_start_matches('\u{feff}').trim_start();
    let ext = if text.contains("<en-export") {
        "enex"
    } else if text.contains("<opml") {
        "opml"
    } else if text.contains("tomboy") && text.contains("<note") {
        "note"
    } else if start.starts_with('{') || start.starts_with('[') {
        "json"
    } else if start.starts_with('<') {
        "html"
    } else {
        return Ok(name.to_owned());
    };
    let stem = name
        .rsplit([':', '/'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("export");
    Ok(format!("{stem}.{ext}"))
}

fn read_up_to(file: &mut File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match file.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    fn write_zip(path: &Path, files: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in files {
            zip.start_file(*name, options).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn folders_are_walked_in_order_without_links() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("a/b")).unwrap();
        std::fs::write(dir.path().join("a/b/note.md"), b"# hi").unwrap();
        std::fs::write(dir.path().join("top.md"), b"top").unwrap();
        let mut source = Source::open(dir.path()).unwrap();
        let paths: Vec<_> = source.entries().iter().map(|e| e.path.clone()).collect();
        assert_eq!(paths, ["a/b/note.md", "top.md"]);
        assert!(source.entries()[0].modified.is_some());
        assert_eq!(source.read("a/b/note.md").unwrap(), b"# hi");
        assert!(source.read("missing.md").is_err());
    }

    #[test]
    fn zips_and_zips_inside_them_read_as_one() {
        let dir = tempfile::tempdir().unwrap();
        let inner = dir.path().join("part.zip");
        write_zip(
            &inner,
            &[("Page abc.md", b"inner page"), ("img/x.png", b"png")],
        );
        let outer = dir.path().join("Export.zip");
        let inner_bytes = std::fs::read(&inner).unwrap();
        write_zip(
            &outer,
            &[
                ("Top.md", b"# Top"),
                ("Export-Part-1.zip", &inner_bytes),
                ("../evil.md", b"no"),
            ],
        );
        let mut source = Source::open(&outer).unwrap();
        let paths: Vec<_> = source.entries().iter().map(|e| e.path.clone()).collect();
        assert_eq!(
            paths,
            [
                "Top.md",
                "Export-Part-1.zip/Page abc.md",
                "Export-Part-1.zip/img/x.png"
            ]
        );
        assert_eq!(
            source.read("Export-Part-1.zip/Page abc.md").unwrap(),
            b"inner page"
        );
        assert_eq!(source.read("Top.md").unwrap(), b"# Top");
        let temp = source.temp.clone();
        drop(source);
        assert!(temp.iter().all(|t| !t.exists()), "temporary zips removed");
    }

    /// Zips inside zips are opened a few
    /// levels deep, no further.
    #[test]
    fn zips_nested_past_three_levels_are_not_opened() {
        let dir = tempfile::tempdir().unwrap();
        let mut bytes = Vec::new();
        for level in (0..6).rev() {
            let path = dir.path().join(format!("l{level}.zip"));
            let note = format!("note {level}");
            if bytes.is_empty() {
                write_zip(&path, &[("deep.md", note.as_bytes())]);
            } else {
                write_zip(&path, &[("deep.md", note.as_bytes()), ("in.zip", &bytes)]);
            }
            bytes = std::fs::read(&path).unwrap();
        }
        let source = Source::open(&dir.path().join("l0.zip")).unwrap();
        let deepest = source
            .entries()
            .iter()
            .map(|e| e.path.matches("in.zip/").count())
            .max()
            .unwrap();
        assert_eq!(deepest, 3, "{:?}", source.entries());
    }

    #[test]
    fn tars_read_by_entry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("export.jex");
        {
            let mut tar = tar::Builder::new(File::create(&path).unwrap());
            for (name, bytes) in [
                ("abc.md", &b"Title\n\nbody"[..]),
                ("resources/r1.png", b"png!"),
            ] {
                let mut header = tar::Header::new_gnu();
                header.set_size(bytes.len() as u64);
                header.set_mode(0o644);
                header.set_mtime(1_700_000_000);
                header.set_cksum();
                tar.append_data(&mut header, name, bytes).unwrap();
            }
            tar.finish().unwrap();
        }
        let mut source = Source::open(&path).unwrap();
        assert_eq!(source.entries().len(), 2);
        assert_eq!(source.entries()[0].modified, Some(1_700_000_000_000));
        assert_eq!(source.read("resources/r1.png").unwrap(), b"png!");
        assert_eq!(source.read("abc.md").unwrap(), b"Title\n\nbody");
    }

    #[test]
    fn entries_are_stored_in_the_vault_files() {
        let dir = tempfile::tempdir().unwrap();
        let zip = dir.path().join("e.zip");
        let big: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        write_zip(&zip, &[("big.bin", &big)]);
        let blobs = crate::blobs::BlobStore::new(dir.path().join("blobs"), None);
        let mut source = Source::open(&zip).unwrap();
        let (sha, size) = source.store("big.bin", &blobs).unwrap();
        assert_eq!(size, 300_000);
        assert_eq!(sha, crate::blobs::sha256_hex(&big));
        assert_eq!(blobs.get(&sha).unwrap().unwrap(), big);
    }

    #[test]
    fn gzip_tars_and_gzip_files_open() {
        let dir = tempfile::tempdir().unwrap();
        let tar_path = dir.path().join("t.tar");
        {
            let mut tar = tar::Builder::new(File::create(&tar_path).unwrap());
            let mut header = tar::Header::new_gnu();
            header.set_size(4);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, "Takeout/Keep/a.json", &b"{}{}"[..])
                .unwrap();
            tar.finish().unwrap();
        }
        let gz = |from: &Path, to: &Path| {
            let mut out = flate2::write::GzEncoder::new(
                File::create(to).unwrap(),
                flate2::Compression::fast(),
            );
            out.write_all(&std::fs::read(from).unwrap()).unwrap();
            out.finish().unwrap();
        };
        let tgz = dir.path().join("takeout.tgz");
        gz(&tar_path, &tgz);
        let mut source = Source::open(&tgz).unwrap();
        assert_eq!(source.entries()[0].path, "Takeout/Keep/a.json");
        assert_eq!(source.read("Takeout/Keep/a.json").unwrap(), b"{}{}");
        let temp = source.temp.clone();
        drop(source);
        assert!(temp.iter().all(|t| !t.exists()));

        let plain = dir.path().join("notes.enex");
        std::fs::write(&plain, b"<en-export/>").unwrap();
        let gzipped = dir.path().join("notes.enex.gz");
        gz(&plain, &gzipped);
        let mut source = Source::open(&gzipped).unwrap();
        assert_eq!(source.entries()[0].path, "notes.enex");
        assert_eq!(source.read("notes.enex").unwrap(), b"<en-export/>");
    }

    #[test]
    fn entries_larger_than_allowed_are_refused_while_read() {
        let dir = tempfile::tempdir().unwrap();
        let zip = dir.path().join("bomb.zip");
        // compresses to a few hundred kilobytes
        let big = vec![0u8; usize::try_from(MAX_READ).unwrap() + 10];
        write_zip(&zip, &[("big.txt", &big)]);
        let mut source = Source::open(&zip).unwrap();
        // what the archive says is believed first, what comes out is checked
        source.entries[0].size = 10;
        assert!(source.read("big.txt").is_err());
    }

    #[test]
    fn handles_without_a_name_are_named_by_what_they_hold() {
        let dir = tempfile::tempdir().unwrap();
        for (content, expected) in [
            (&b"<?xml version=\"1.0\"?><en-export>"[..], "12.enex"),
            (b"  [{\"title\": \"x\"}]", "12.json"),
            (b"<opml version=\"2.0\">", "12.opml"),
            (b"binary\x00stuff", "msf:12"),
        ] {
            let path = dir.path().join("doc");
            std::fs::write(&path, content).unwrap();
            let source = Source::open_file(File::open(&path).unwrap(), "msf:12").unwrap();
            assert_eq!(source.entries()[0].path, expected);
        }
        // a zip from a handle reads like one from a path
        let zip = dir.path().join("z");
        write_zip(&zip, &[("a.md", b"x")]);
        let mut source = Source::open_file(File::open(&zip).unwrap(), "12").unwrap();
        assert_eq!(source.read("a.md").unwrap(), b"x");
    }

    #[test]
    fn a_single_file_is_its_own_entry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("My Notebook.enex");
        std::fs::write(&path, b"<?xml version=\"1.0\"?><en-export/>").unwrap();
        let mut source = Source::open(&path).unwrap();
        assert_eq!(source.entries()[0].path, "My Notebook.enex");
        assert!(
            source
                .read("My Notebook.enex")
                .unwrap()
                .starts_with(b"<?xml")
        );
    }
}
