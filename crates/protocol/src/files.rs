// SPDX-License-Identifier: AGPL-3.0-or-later

//! Attachment files on the server: their state during an upload, the
//! policy that applies to an account, and how much it uses.
//!
//! A file is sent in parts with `PUT /api/v1/sync/files/{address}?space=`
//! and a `Content-Range` header, read with `GET` (a `Range` header resumes),
//! and released with `DELETE ...?space=&head=`. The address is the SHA-256
//! of the content for standard accounts, a keyed hash for e2ee ones.

use serde::{Deserialize, Serialize};

/// Largest part of an upload, in bytes.
pub const MAX_PART: u64 = 8 * 1024 * 1024;

/// Where a file stands on the server for one space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FileStatus {
    /// Nothing of it for this space.
    None,
    /// An upload started, `received` bytes are in.
    Partial,
    /// The space has it.
    Complete,
    /// The space let it go not long ago and it is still kept: `POST
    /// .../keep` takes it back without sending it again.
    Kept,
}

/// `GET /api/v1/sync/files/{address}/state?space=`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileState {
    /// Where it stands.
    pub status: FileStatus,
    /// Bytes received of a partial upload.
    pub received: u64,
    /// Its whole size, 0 when unknown.
    pub size: u64,
}

/// The rules for an account's files, after user, group and instance
/// settings are combined. A limit of 0 means no limit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesPolicy {
    /// Largest file, in bytes.
    pub max_file_bytes: u64,
    /// Total storage of the spaces the account owns, in bytes.
    pub quota_bytes: u64,
    /// Types allowed (extensions like `.pdf`, MIME patterns like
    /// `image/*`), empty for every type not blocked.
    pub allowed_types: Vec<String>,
    /// Types refused, checked after the allowed ones.
    pub blocked_types: Vec<String>,
    /// Attachments a note may have.
    pub max_files_per_note: u32,
    /// Longest side images are shrunk to when attached, 0 to keep them.
    pub image_max_px: u32,
    /// Quality of shrunk images, 1 to 100.
    pub image_quality: u32,
    /// Whether apps make a small preview of each image.
    pub thumbnails: bool,
    /// Days a removed file is kept before it is gone.
    pub retention_days: u32,
}

/// What an account uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesUsage {
    /// Bytes counted against the quota: files and notes of the spaces the
    /// account owns.
    pub used_bytes: u64,
    /// Of which files.
    pub file_bytes: u64,
    /// Number of files.
    pub files: u64,
}

/// `GET /api/v1/sync/files/policy`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesInfo {
    /// The rules in effect for this account.
    pub policy: FilesPolicy,
    /// What it uses.
    pub usage: FilesUsage,
    /// Largest part an upload may send, a whole number of 64 KB chunks.
    pub part_bytes: u64,
}

/// Whether a rule (`.pdf`, `image/*`, `application/pdf`) covers a file.
fn covers(rule: &str, name: &str, mime: &str) -> bool {
    let rule = rule.trim().to_lowercase();
    if rule.is_empty() {
        return false;
    }
    if rule.starts_with('.') {
        return name.to_lowercase().ends_with(&rule);
    }
    let mime = mime.to_lowercase();
    match rule.strip_suffix("/*") {
        Some(family) => mime.split('/').next() == Some(family),
        None => mime == rule,
    }
}

/// Whether a file of this name and type may be stored: allowed when the
/// allowed list is empty or covers it, and no blocked rule covers it.
pub fn allows(policy: &FilesPolicy, name: &str, mime: &str) -> bool {
    let allowed = policy.allowed_types.is_empty()
        || policy.allowed_types.iter().any(|r| covers(r, name, mime));
    allowed && !policy.blocked_types.iter().any(|r| covers(r, name, mime))
}

/// Request header with an upload's file name, percent encoded (header
/// values are ASCII). Standard accounts send it with the first part so the
/// server can check the type, e2ee accounts never do.
pub const NAME_HEADER: &str = "x-file-name";

/// Request header with an upload's declared type, sent with the name.
pub const TYPE_HEADER: &str = "x-file-type";

/// Percent encodes a file name for [`NAME_HEADER`]: everything but ASCII
/// letters, digits and `-._~`.
pub fn encode_name(name: &str) -> String {
    use std::fmt::Write as _;
    name.bytes().fold(String::new(), |mut out, b| {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            // writing to a String cannot fail
            let _ = write!(out, "%{b:02X}");
        }
        out
    })
}

/// Decodes `%XX` escapes, `None` for a malformed one or text that is not
/// UTF-8.
pub fn decode_name(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_survive_the_header() {
        for name in ["vigne été.png", "a%b.pdf", "plain.txt", "日本.jpg"] {
            let encoded = encode_name(name);
            assert!(encoded.is_ascii());
            assert_eq!(decode_name(&encoded).as_deref(), Some(name));
        }
        assert_eq!(decode_name("bad%zz"), None);
        assert_eq!(decode_name("cut%4"), None);
    }
}
