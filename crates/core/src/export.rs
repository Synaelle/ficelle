// SPDX-License-Identifier: AGPL-3.0-or-later

//! Export to a plain folder of markdown files that any editor can open.
//!
//! - The folder tree becomes directories.
//! - Each note becomes `<title>.md`, with YAML front matter for the id,
//!   title, tags, pin and dates.
//! - Attachments go to an `attachments/` directory next to their note, so a
//!   body link like `attachments/vines.png` keeps working without rewriting.
//! - Notes and folders in the trash are left out.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::clock::to_rfc3339;
use crate::error::{Error, Result};
use crate::id::FolderId;
use crate::model::{Folder, Note, NoteScope, NoteSort};
use crate::store::Store;

/// Where attachment content comes from, by SHA-256.
pub trait AttachmentSource {
    /// Reads a file, `None` when it is not available on this device.
    ///
    /// # Errors
    ///
    /// Fails on read errors.
    fn read(&self, sha256: &str) -> Result<Option<Vec<u8>>>;
}

/// No attachment content, only notes are exported.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoAttachments;

impl AttachmentSource for NoAttachments {
    fn read(&self, _sha256: &str) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }
}

/// What an export wrote.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ExportReport {
    /// Notes written.
    pub notes: usize,
    /// Directories created for folders.
    pub folders: usize,
    /// Attachment files written.
    pub attachments: usize,
    /// Attachments that could not be found, as `note title: file name`.
    pub missing_attachments: Vec<String>,
}

/// Exports every live note and folder into `out`, which must be missing or empty.
///
/// # Errors
///
/// Fails when `out` is not empty, or on database and file errors.
pub fn export_markdown(
    store: &Store,
    out: &Path,
    files: &dyn AttachmentSource,
) -> Result<ExportReport> {
    if out.exists() && std::fs::read_dir(out)?.next().is_some() {
        return Err(Error::Invalid(
            "choose an empty folder for the export".into(),
        ));
    }
    std::fs::create_dir_all(out)?;

    let mut report = ExportReport::default();
    let mut names = UsedNames::default();
    let folder_dirs = create_folder_dirs(store, out, &mut names, &mut report)?;

    for summary in store.list_notes(NoteScope::All, NoteSort::TitleAsc)? {
        let note = store.note(summary.id)?;
        let dir = note
            .folder_id
            .and_then(|f| folder_dirs.get(&f))
            .cloned()
            .unwrap_or_else(|| out.to_owned());
        let file = names.claim(&dir, &format!("{}.md", safe_name(&note.title)));
        // files first: one renamed on disk (a name taken by another note of
        // the folder, a character the system refuses) is renamed in the text
        let renamed = export_attachments(store, &note, &dir, files, &mut names, &mut report)?;
        std::fs::write(&file, render_note(&note, &relink(&note.body, &renamed)))?;
        report.notes += 1;
    }
    Ok(report)
}

fn create_folder_dirs(
    store: &Store,
    out: &Path,
    names: &mut UsedNames,
    report: &mut ExportReport,
) -> Result<HashMap<FolderId, PathBuf>> {
    let folders = store.list_folders()?;
    let by_id: HashMap<FolderId, &Folder> = folders.iter().map(|f| (f.id, f)).collect();
    let mut dirs = HashMap::new();
    for folder in &folders {
        let dir = dir_for(folder.id, &by_id, out, names, &mut dirs);
        std::fs::create_dir_all(&dir)?;
        report.folders += 1;
    }
    Ok(dirs)
}

/// Directory for a folder, creating parent entries first. Parents are
/// resolved on demand, so the order of the folder list does not matter.
fn dir_for(
    id: FolderId,
    by_id: &HashMap<FolderId, &Folder>,
    out: &Path,
    names: &mut UsedNames,
    dirs: &mut HashMap<FolderId, PathBuf>,
) -> PathBuf {
    if let Some(dir) = dirs.get(&id) {
        return dir.clone();
    }
    let folder = by_id[&id];
    let parent = folder
        .parent_id
        .filter(|p| by_id.contains_key(p))
        .map_or_else(|| out.to_owned(), |p| dir_for(p, by_id, out, names, dirs));
    let dir = names.claim(&parent, &safe_name(&folder.name));
    dirs.insert(id, dir.clone());
    dir
}

fn export_attachments(
    store: &Store,
    note: &Note,
    dir: &Path,
    files: &dyn AttachmentSource,
    names: &mut UsedNames,
    report: &mut ExportReport,
) -> Result<Vec<(String, String)>> {
    let mut renamed = Vec::new();
    let attachments = store.list_attachments(note.id)?;
    if attachments.is_empty() {
        return Ok(renamed);
    }
    let target = dir.join("attachments");
    std::fs::create_dir_all(&target)?;
    for attachment in attachments {
        match files.read(&attachment.sha256)? {
            Some(bytes) => {
                let path = names.claim(&target, &safe_name(&attachment.file_name));
                let written = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if written != attachment.file_name {
                    renamed.push((attachment.file_name.clone(), written));
                }
                std::fs::write(path, bytes)?;
                report.attachments += 1;
            }
            None => report
                .missing_attachments
                .push(format!("{}: {}", note.title, attachment.file_name)),
        }
    }
    Ok(renamed)
}

/// A file name as the editor writes it in a link (`encodeURI`).
fn encode_uri(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric() || b";,/?:@&=+$-_.!~*'()#".contains(&byte) {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// The body with its links to renamed files pointing at their new names.
fn relink(body: &str, renamed: &[(String, String)]) -> String {
    let mut out = body.to_owned();
    for (from, to) in renamed {
        let new = format!(
            "](attachments/{})",
            encode_uri(to).replace('(', "%28").replace(')', "%29")
        );
        for old in [encode_uri(from), from.clone()] {
            out = out.replace(&format!("](attachments/{old})"), &new);
        }
    }
    out
}

/// Front matter then body.
fn render_note(note: &Note, body: &str) -> String {
    let mut text = String::from("---\n");
    let _ = writeln!(text, "id: {}", note.id);
    let _ = writeln!(text, "title: {}", yaml_string(&note.title));
    let tags = note
        .tags
        .iter()
        .map(|t| yaml_string(t))
        .collect::<Vec<_>>()
        .join(", ");
    let _ = writeln!(text, "tags: [{tags}]");
    let _ = writeln!(text, "pinned: {}", note.pinned);
    let _ = writeln!(text, "created: {}", to_rfc3339(note.created_at));
    let _ = writeln!(text, "updated: {}", to_rfc3339(note.updated_at));
    text.push_str("---\n\n");
    text.push_str(body);
    if !body.ends_with('\n') {
        text.push('\n');
    }
    text
}

/// A double quoted YAML scalar. Quoting everything avoids YAML surprises such
/// as `no` turning into false or `1.0` into a number.
fn yaml_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A file or directory name that is valid on Windows, macOS and Linux.
fn safe_name(raw: &str) -> String {
    const MAX_CHARS: usize = 100;
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_control() || r#"<>:"/\|?*"#.contains(c) {
                ' '
            } else {
                c
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let name: String = cleaned.chars().take(MAX_CHARS).collect();
    // Windows drops trailing dots and spaces, which would make names collide
    let mut name = name.trim_end_matches(['.', ' ']).trim_start().to_owned();
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if name.is_empty() || RESERVED.contains(&stem.as_str()) {
        name.insert_str(0, "Untitled ");
        name.truncate(name.trim_end().len());
    }
    name
}

/// Remembers names already used per directory, ignoring case because the
/// default macOS and Windows file systems do.
#[derive(Default)]
struct UsedNames(HashMap<PathBuf, HashSet<String>>);

impl UsedNames {
    /// Returns a free path for `name` in `dir`, adding ` 2`, ` 3`, ... before
    /// the extension when needed.
    fn claim(&mut self, dir: &Path, name: &str) -> PathBuf {
        let used = self.0.entry(dir.to_owned()).or_default();
        let (stem, ext) = match name.rsplit_once('.') {
            Some((stem, ext)) if !stem.is_empty() => (stem.to_owned(), format!(".{ext}")),
            _ => (name.to_owned(), String::new()),
        };
        let mut candidate = name.to_owned();
        let mut n = 2;
        while !used.insert(candidate.to_lowercase()) {
            candidate = format!("{stem} {n}{ext}");
            n += 1;
        }
        dir.join(candidate)
    }
}

/// The same export in one zip file: written to a folder in the system's
/// temporary files, zipped beside `out`, moved in place, the folder
/// removed.
///
/// # Errors
///
/// As [`export_markdown`], and zip errors.
pub fn export_markdown_zip(
    store: &Store,
    out: &Path,
    files: &dyn AttachmentSource,
) -> Result<ExportReport> {
    let partial = out.with_extension("partial");
    let done = std::fs::File::create(&partial)
        .map_err(Error::from)
        .and_then(|file| export_markdown_zip_to(store, file, files));
    match done {
        Ok(report) => {
            std::fs::rename(&partial, out)?;
            Ok(report)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&partial);
            Err(e)
        }
    }
}

/// The zip export into a file already open (an Android document).
///
/// # Errors
///
/// As [`export_markdown_zip`].
pub fn export_markdown_zip_to(
    store: &Store,
    file: std::fs::File,
    files: &dyn AttachmentSource,
) -> Result<ExportReport> {
    // the notes in plain text a moment: in a folder only this user reads
    // (0700 on Linux), gone when done or when the export fails
    let staging = tempfile::Builder::new().prefix("export-").tempdir()?;
    let temp = staging.path().to_path_buf();
    let done = (|| -> Result<ExportReport> {
        let report = export_markdown(store, &temp, files)?;
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .large_file(true);
        let mut dirs = vec![temp.clone()];
        while let Some(dir) = dirs.pop() {
            let mut entries: Vec<_> = std::fs::read_dir(&dir)?.collect::<std::io::Result<_>>()?;
            entries.sort_by_key(std::fs::DirEntry::file_name);
            for entry in entries {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                    continue;
                }
                let name = path
                    .strip_prefix(&temp)
                    .map_err(|_| Error::Invalid("export path".into()))?
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                zip.start_file(name, options)
                    .map_err(|e| Error::Invalid(e.to_string()))?;
                std::io::copy(&mut std::fs::File::open(&path)?, &mut zip)?;
            }
        }
        zip.finish().map_err(|e| Error::Invalid(e.to_string()))?;
        Ok(report)
    })();
    drop(staging);
    done
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::NewNote;
    use crate::store::test_support::store;

    struct OneFile;
    impl AttachmentSource for OneFile {
        fn read(&self, sha256: &str) -> Result<Option<Vec<u8>>> {
            Ok(sha256.starts_with('a').then(|| b"png bytes".to_vec()))
        }
    }

    #[test]
    fn names_are_safe_everywhere() {
        assert_eq!(safe_name("a/b:c*d?"), "a b c d");
        assert_eq!(safe_name("  trailing dots... "), "trailing dots");
        assert_eq!(safe_name(""), "Untitled");
        assert_eq!(safe_name("con"), "Untitled con");
        assert_eq!(safe_name("CON.txt"), "Untitled CON.txt");
        assert_eq!(safe_name(&"x".repeat(300)).len(), 100);
    }

    #[test]
    fn yaml_strings_are_escaped() {
        assert_eq!(yaml_string("say \"no\"\n\\"), r#""say \"no\"\n\\""#);
        assert_eq!(yaml_string("no"), "\"no\"");
    }

    #[test]
    fn duplicate_names_get_numbers_ignoring_case() {
        let mut names = UsedNames::default();
        let dir = Path::new("out");
        assert_eq!(names.claim(dir, "Note.md"), dir.join("Note.md"));
        assert_eq!(names.claim(dir, "note.md"), dir.join("note 2.md"));
        assert_eq!(names.claim(dir, "Note.md"), dir.join("Note 3.md"));
        assert_eq!(names.claim(dir, "Folder"), dir.join("Folder"));
        assert_eq!(names.claim(dir, "Folder"), dir.join("Folder 2"));
    }

    /// Files whose content is their name, for the export to write.
    struct NamedFiles;
    impl AttachmentSource for NamedFiles {
        fn read(&self, sha256: &str) -> Result<Option<Vec<u8>>> {
            Ok(Some(sha256.as_bytes().to_vec()))
        }
    }

    #[test]
    fn files_renamed_on_disk_are_renamed_in_the_text() {
        let (mut store, _) = store();
        let mut write = |title: &str, file: &str, sha: &str| {
            let note = store
                .create_note(&NewNote {
                    title: title.into(),
                    ..NewNote::default()
                })
                .unwrap();
            let a = store
                .add_attachment(note.id, file, "image/png", 1, sha)
                .unwrap();
            let link = format!("![x](attachments/{}){{width=10}}", encode_uri(&a.file_name));
            store
                .update_note(
                    note.id,
                    crate::store::NoteChange {
                        body: Some(link),
                        ..Default::default()
                    },
                )
                .unwrap();
        };
        // two notes of one folder with a file of the same name, and a name
        // the system refuses
        write("A", "photo one.png", &"a".repeat(64));
        write("B", "photo one.png", &"b".repeat(64));
        write("C", "what: now?.png", &"c".repeat(64));
        let dir = tempfile::tempdir().unwrap();
        export_markdown(&store, dir.path(), &NamedFiles).unwrap();
        for (note, sha) in [("A", "a"), ("B", "b"), ("C", "c")] {
            let text = std::fs::read_to_string(dir.path().join(format!("{note}.md"))).unwrap();
            let link = text
                .split("](attachments/")
                .nth(1)
                .and_then(|rest| rest.split(')').next())
                .unwrap()
                .replace("%20", " ")
                .replace("%28", "(")
                .replace("%29", ")");
            let bytes = std::fs::read(dir.path().join("attachments").join(&link)).unwrap();
            assert_eq!(bytes, sha.repeat(64).into_bytes(), "{note} links to {link}");
            assert!(text.contains("){width=10}"), "{text}");
        }
    }

    #[test]
    fn exports_into_one_zip_without_leaving_files_behind() {
        let (mut store, _) = store();
        let folder = store.create_folder(None, "Garden").unwrap().id;
        store
            .create_note(&NewNote {
                folder_id: Some(folder),
                title: "Beds".into(),
                body: "dig".into(),
                ..NewNote::default()
            })
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("notes.zip");
        let report = export_markdown_zip(&store, &out, &NoAttachments).unwrap();
        assert_eq!(report.notes, 1);
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
        let mut text = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("Garden/Beds.md").unwrap(), &mut text)
            .unwrap();
        assert!(text.ends_with("dig\n"));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn exports_tree_front_matter_and_attachments() {
        let (mut store, _) = store();
        let projects = store.create_folder(None, "Projects").unwrap();
        let garden = store.create_folder(Some(projects.id), "Garden").unwrap();
        let note = store
            .create_note(&NewNote {
                folder_id: Some(garden.id),
                title: "Tomato trellis plan".into(),
                body: "![vines](attachments/vines.png)".into(),
                tags: vec!["garden".into()],
                pinned: true,
            })
            .unwrap();
        store
            .create_note(&NewNote {
                title: "Tomato trellis plan".into(),
                folder_id: Some(garden.id),
                ..NewNote::default()
            })
            .unwrap();
        store
            .create_note(&NewNote {
                title: "Top level".into(),
                ..NewNote::default()
            })
            .unwrap();
        let gone = store
            .create_note(&NewNote {
                title: "Deleted".into(),
                ..NewNote::default()
            })
            .unwrap();
        store.trash_note(gone.id).unwrap();
        store
            .add_attachment(note.id, "vines.png", "image/png", 9, &"a".repeat(64))
            .unwrap();
        store
            .add_attachment(note.id, "lost.pdf", "application/pdf", 9, &"b".repeat(64))
            .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("export");
        let report = export_markdown(&store, &out, &OneFile).unwrap();
        assert_eq!(report.notes, 3);
        assert_eq!(report.folders, 2);
        assert_eq!(report.attachments, 1);
        assert_eq!(
            report.missing_attachments,
            vec!["Tomato trellis plan: lost.pdf"]
        );

        let garden_dir = out.join("Projects").join("Garden");
        let text = std::fs::read_to_string(garden_dir.join("Tomato trellis plan.md")).unwrap();
        assert!(text.starts_with(&format!(
            "---\nid: {}\ntitle: \"Tomato trellis plan\"\ntags: [\"garden\"]\npinned: true\n",
            note.id
        )));
        assert!(text.ends_with("---\n\n![vines](attachments/vines.png)\n"));
        assert!(garden_dir.join("Tomato trellis plan 2.md").is_file());
        assert_eq!(
            std::fs::read(garden_dir.join("attachments").join("vines.png")).unwrap(),
            b"png bytes"
        );
        assert!(out.join("Top level.md").is_file());
        assert!(!out.join("Deleted.md").exists());

        assert!(
            export_markdown(&store, &out, &NoAttachments).is_err(),
            "refuses a non empty folder"
        );
    }
}
