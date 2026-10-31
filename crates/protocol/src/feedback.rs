// SPDX-License-Identifier: AGPL-3.0-or-later

//! Feedback and bug reports, sent from the apps to an instance that receives
//! them, without an account. Before a report the app asks for a challenge
//! and finds a nonce such that SHA-256 of the challenge's text then the
//! nonce (8 bytes, little endian) starts with `bits` zero bits: a fraction
//! of a second for one report, slow for a flood.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Longest about line, in characters.
pub const MAX_ABOUT: usize = 120;
/// Longest message, in characters.
pub const MAX_MESSAGE: usize = 5_000;
/// Longest logs, in bytes.
pub const MAX_LOGS: usize = 64 * 1024;
/// Most images in one report.
pub const MAX_IMAGES: usize = 3;
/// Largest image, in bytes once decoded.
pub const MAX_IMAGE_BYTES: usize = 2 * 1024 * 1024;
/// Largest image side, in pixels.
pub const MAX_IMAGE_SIDE: u32 = 4_096;
/// Longest contact, an email address or a Discord name.
pub const MAX_CONTACT: usize = 254;

/// Where an instance's people send their reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SendTo {
    /// The project's main instance, the brand's `feedbackUrl`.
    #[default]
    Main,
    /// This instance, which receives reports.
    Here,
    /// Nowhere, the apps hide Send feedback.
    Off,
}

/// `GET /api/v1/feedback/info`, open to anyone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackInfo {
    /// This instance takes reports.
    pub receive: bool,
    /// Where this instance's people send theirs. Never `here` while
    /// `receive` is off.
    pub send_to: SendTo,
}

/// What a report was about, from apps before 0.2.0. Newer ones say it in
/// words, see [`Report::about`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    /// Something does not work.
    Bug,
    /// Something to add or change.
    Idea,
    /// How to do something.
    Question,
    /// Anything else.
    Other,
}

impl Category {
    /// As stored and shown.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bug => "bug",
            Self::Idea => "idea",
            Self::Question => "question",
            Self::Other => "other",
        }
    }
}

/// `POST /api/v1/feedback/challenge`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Challenge {
    /// Sent back with the report, hashed as its text.
    pub challenge: String,
    /// Zero bits the hash must start with.
    pub bits: u8,
    /// When it stops being accepted, unix milliseconds.
    pub expires_at: i64,
}

/// `POST /api/v1/feedback`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    /// From [`Challenge`].
    pub challenge: String,
    /// What [`solve`] found.
    pub nonce: u64,
    /// A random id the app made, 32 lowercase hex, tied to nothing else.
    #[serde(default)]
    pub install: Option<String>,
    /// What it is about, from apps before 0.2.0.
    #[serde(default)]
    pub category: Option<Category>,
    /// What it is about in one line, from apps since 0.2.0.
    #[serde(default)]
    pub about: Option<String>,
    /// The person's words, may be empty when `about` says it all.
    #[serde(default)]
    pub message: String,
    /// Where to answer, an email address or a Discord name, when the person
    /// wants an answer. `email` from apps before 0.2.0.
    #[serde(default, alias = "email")]
    pub contact: Option<String>,
    /// The app's version.
    pub version: String,
    /// `windows`, `android`, `web`...
    pub platform: String,
    /// Recent errors and warnings, when the person attached them.
    #[serde(default)]
    pub logs: Option<String>,
    /// PNG, JPEG or WebP images, standard base64.
    #[serde(default)]
    pub images: Vec<String>,
    /// The host of the server the app is signed in to, none for an app on
    /// no server. The receiving instance counts its caps per server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    /// SHA-256, lowercase hex, of a secret the app keeps: with it, the app
    /// alone asks how this report stands ([`AnswersRequest`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<String>,
}

/// `POST /api/v1/feedback/answers`: how the app's own reports stand, by
/// the secrets of their receipts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswersRequest {
    /// At most [`MAX_ASKED`].
    pub secrets: Vec<String>,
}

/// The most reports asked about at once.
pub const MAX_ASKED: usize = 50;

/// How a report stands, as its sender sees it. A report filed as spam reads
/// as received, so a sender learns nothing of the filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SentState {
    /// Taken in, not looked at yet.
    Received,
    /// Being looked at.
    Open,
    /// Dealt with.
    Done,
}

/// One report the server still has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SentReport {
    /// The secret it was asked by.
    pub secret: String,
    /// How it stands.
    pub state: SentState,
    /// The moderators' one answer, empty until they give it.
    pub answer: String,
}

/// The answer to [`AnswersRequest`]: a report the server no longer has
/// (kept long enough, or deleted) is missing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Answers {
    /// The reports still there.
    pub reports: Vec<SentReport>,
}

/// Zero bits at the start of `hash`.
fn leading_zero_bits(hash: &[u8]) -> u32 {
    let mut bits = 0;
    for byte in hash {
        if *byte == 0 {
            bits += 8;
        } else {
            return bits + byte.leading_zeros();
        }
    }
    bits
}

/// Whether `nonce` answers `challenge` at `bits`.
pub fn meets(challenge: &str, nonce: u64, bits: u8) -> bool {
    let hash = Sha256::new()
        .chain_update(challenge.as_bytes())
        .chain_update(nonce.to_le_bytes())
        .finalize();
    leading_zero_bits(&hash) >= u32::from(bits)
}

/// The first nonce that answers `challenge`, about 2^`bits` tries.
pub fn solve(challenge: &str, bits: u8) -> u64 {
    // the prefix is hashed once, each try only adds the nonce
    let prefix = Sha256::new().chain_update(challenge.as_bytes());
    (0..=u64::MAX)
        .find(|nonce| {
            let hash = prefix.clone().chain_update(nonce.to_le_bytes()).finalize();
            leading_zero_bits(&hash) >= u32::from(bits)
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_solved_challenge_meets_its_bits_and_others_do_not() {
        let nonce = solve("abc", 12);
        assert!(meets("abc", nonce, 12));
        assert!(
            (0..nonce).all(|n| !meets("abc", n, 12)),
            "the first one found"
        );
        assert!(meets("anything", 0, 0));
        assert_eq!(leading_zero_bits(&[0, 0x10, 0xff]), 11);
        assert_eq!(leading_zero_bits(&[0, 0]), 16);
    }

    #[test]
    fn reports_read_with_their_optional_parts_left_out() {
        let report: Report = serde_json::from_str(
            r#"{"challenge":"c","nonce":7,"category":"bug","message":"m","version":"1","platform":"web"}"#,
        )
        .unwrap();
        assert_eq!(report.install, None);
        assert_eq!(report.category, Some(Category::Bug));
        assert!(report.images.is_empty());
        // an older app's email is the contact, a newer app sends no category
        let report: Report = serde_json::from_str(
            r#"{"challenge":"c","nonce":7,"about":"a","email":"k@x.org","version":"1","platform":"web"}"#,
        )
        .unwrap();
        assert_eq!(report.category, None);
        assert_eq!(report.message, "");
        assert_eq!(report.contact.as_deref(), Some("k@x.org"));
        let info = serde_json::to_string(&FeedbackInfo {
            receive: true,
            send_to: SendTo::Here,
        })
        .unwrap();
        assert_eq!(info, r#"{"receive":true,"sendTo":"here"}"#);
    }
}
