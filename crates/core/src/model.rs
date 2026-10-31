// SPDX-License-Identifier: AGPL-3.0-or-later

//! Plain data types returned by the store.

use serde::{Deserialize, Serialize};

use crate::clock::Timestamp;
use crate::id::{AttachmentId, FolderId, NoteId, TagId, VersionId};

/// A note. The body is markdown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    /// Unique id.
    pub id: NoteId,
    /// The space it belongs to: empty for the personal space, else a
    /// shared space's id.
    pub space_id: String,
    /// Folder holding the note, `None` for the top level.
    pub folder_id: Option<FolderId>,
    /// Title, may be empty.
    pub title: String,
    /// Markdown content.
    pub body: String,
    /// Shown above other notes in lists.
    pub pinned: bool,
    /// Tag names, sorted.
    pub tags: Vec<String>,
    /// When the note was created.
    pub created_at: Timestamp,
    /// Last change to title, body, tags, pin or folder.
    pub updated_at: Timestamp,
    /// Set while the note is in the trash.
    pub trashed_at: Option<Timestamp>,
}

/// A short form of a note for lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteSummary {
    /// Unique id.
    pub id: NoteId,
    /// Folder holding the note.
    pub folder_id: Option<FolderId>,
    /// Title, may be empty.
    pub title: String,
    /// First characters of the body, without line breaks.
    pub snippet: String,
    /// Shown above other notes.
    pub pinned: bool,
    /// Last change.
    pub updated_at: Timestamp,
    /// Creation time.
    pub created_at: Timestamp,
    /// The space it belongs to, empty for the personal space.
    pub space_id: String,
}

/// A folder. Folders nest without limit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    /// Unique id.
    pub id: FolderId,
    /// Parent folder, `None` for the top level.
    pub parent_id: Option<FolderId>,
    /// Display name.
    pub name: String,
    /// Order among siblings, lower first.
    pub position: i64,
    /// When the folder was created.
    pub created_at: Timestamp,
    /// Last rename or move.
    pub updated_at: Timestamp,
    /// Set while the folder is in the trash.
    pub trashed_at: Option<Timestamp>,
    /// The space it belongs to, empty for the personal space.
    pub space_id: String,
}

/// A tag with the number of live notes using it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    /// Unique id.
    pub id: TagId,
    /// Name as first typed. Names are unique ignoring case.
    pub name: String,
    /// Live notes (not in the trash) with this tag.
    pub note_count: i64,
}

/// Why a version was saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VersionReason {
    /// Regular editing, at most one version per editing session.
    Edit,
    /// Saved just before an older version was restored, so the restore can be undone.
    BeforeRestore,
    /// This device's text just before a suspicious merge.
    MineBeforeMerge,
    /// The other device's text just before a suspicious merge.
    TheirsBeforeMerge,
}

impl VersionReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Edit => "edit",
            Self::BeforeRestore => "before-restore",
            Self::MineBeforeMerge => "mine-before-merge",
            Self::TheirsBeforeMerge => "theirs-before-merge",
        }
    }

    pub(crate) fn parse(text: &str) -> Self {
        match text {
            "before-restore" => Self::BeforeRestore,
            "mine-before-merge" => Self::MineBeforeMerge,
            "theirs-before-merge" => Self::TheirsBeforeMerge,
            _ => Self::Edit,
        }
    }
}

/// An earlier state of a note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteVersion {
    /// Unique id.
    pub id: VersionId,
    /// Note this version belongs to.
    pub note_id: NoteId,
    /// Title at that time.
    pub title: String,
    /// Body at that time.
    pub body: String,
    /// When this state was saved.
    pub created_at: Timestamp,
    /// Why it was saved.
    pub reason: VersionReason,
}

/// Description of a file attached to a note. The file itself is stored
/// elsewhere, addressed by its hash (phase 8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    /// Unique id.
    pub id: AttachmentId,
    /// Owning note.
    pub note_id: NoteId,
    /// Original file name.
    pub file_name: String,
    /// Media type, for example `image/png`.
    pub mime_type: String,
    /// Size in bytes.
    pub size: i64,
    /// Hex SHA-256 of the content, the storage address.
    pub sha256: String,
    /// When it was attached.
    pub created_at: Timestamp,
    /// Hex SHA-256 of a small preview image, for images.
    pub thumb_sha256: Option<String>,
}

/// An attachment with the title of its note, for the list of every file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteAttachment {
    /// The attachment.
    #[serde(flatten)]
    pub attachment: Attachment,
    /// Its note's title.
    pub note_title: String,
}

/// How note lists are ordered. Pinned notes always come first.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NoteSort {
    /// Most recently changed first.
    #[default]
    UpdatedDesc,
    /// Newest first.
    CreatedDesc,
    /// Alphabetical by title.
    TitleAsc,
}

/// Which notes a list shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NoteScope {
    /// Every live note.
    #[default]
    All,
    /// Live notes directly in one folder, or at the top level for `None`.
    Folder(Option<FolderId>),
    /// Live notes with this tag.
    Tag(TagId),
    /// Notes in the trash.
    Trash,
}

/// A search result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    /// Matching note.
    pub note_id: NoteId,
    /// Its title.
    pub title: String,
    /// Body excerpt around the match. Matched words are wrapped in
    /// [`MATCH_START`] and [`MATCH_END`] so the ui can highlight them.
    pub snippet: String,
}

/// Marks the start of a matched word in [`SearchHit::snippet`]. Control
/// characters are stripped from notes on save, so this never clashes with content.
pub const MATCH_START: char = '\u{2}';
/// Marks the end of a matched word in [`SearchHit::snippet`].
pub const MATCH_END: char = '\u{3}';
