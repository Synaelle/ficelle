// SPDX-License-Identifier: AGPL-3.0-or-later

//! The signed list of releases servers check for update notices. Each entry
//! is signed on its own with the owner's Ed25519 key, over a message that
//! starts with `releases-{FORMAT}`: a catalog signature (which starts with
//! `catalog-`) never passes for a release, even under the same key.

use serde::{Deserialize, Serialize};

/// Format of `releases.json` this build reads.
pub const FORMAT: u32 = 1;

/// `releases.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseList {
    /// [`FORMAT`]. Absent in a list written by hand, set when it is signed.
    #[serde(default)]
    pub format: u32,
    /// Releases, any order.
    pub releases: Vec<Release>,
}

/// One release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    /// `major.minor.micro`.
    pub version: String,
    /// `YYYY-MM-DD`.
    pub date: String,
    /// It fixes a security problem: servers flag it.
    #[serde(default)]
    pub security: bool,
    /// One line on what changed.
    #[serde(default)]
    pub notes: String,
    /// Where to read more.
    #[serde(default)]
    pub url: String,
    /// Hex Ed25519 signature of [`signed_message`].
    #[serde(default)]
    pub signature: String,
}

fn json(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_default()
}

/// What the signature covers, every field but itself.
pub fn signed_message(release: &Release) -> Vec<u8> {
    [
        format!("releases-{FORMAT}"),
        json(&release.version),
        json(&release.date),
        release.security.to_string(),
        json(&release.notes),
        json(&release.url),
    ]
    .join("\n")
    .into_bytes()
}

/// `major.minor.micro` as numbers. `None` for anything else: a dev or
/// preview build compares with nothing.
pub fn version_parts(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.trim().split('.').map(|p| {
        (!p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
            .then(|| p.parse::<u64>().ok())
            .flatten()
    });
    let found = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(found)
}

/// `GET /api/v1/admin/updates`: the update check as the admin sees it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent facts the admin page shows, not a state"
)]
pub struct UpdatesView {
    /// The admin's switch.
    pub enabled: bool,
    /// The server can check: `[updates] check` and a key.
    pub available: bool,
    /// This server's version.
    pub current: String,
    /// The newest signed release, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest: Option<Release>,
    /// A release newer than this one exists.
    pub newer: bool,
    /// One of the releases newer than this one fixes a security problem.
    pub security: bool,
    /// Last check, unix milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_at: Option<i64>,
    /// Why the last check failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// `PUT /api/v1/admin/updates`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatesRequest {
    /// Check once a day.
    pub enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_as_numbers_and_builds_not_at_all() {
        assert_eq!(version_parts("0.1.0"), Some((0, 1, 0)));
        assert!(version_parts("0.10.0") > version_parts("0.9.9"));
        for odd in [
            "dev-064ce22d6-dirty",
            "preview-abc",
            "1.2",
            "1.2.3.4",
            "1.x.3",
            "",
            "1..3",
        ] {
            assert_eq!(version_parts(odd), None, "{odd}");
        }
    }

    #[test]
    fn a_catalog_signature_message_is_never_a_release_one() {
        let release = Release {
            version: "0.1.0".into(),
            date: "2026-11-01".into(),
            security: false,
            notes: String::new(),
            url: String::new(),
            signature: String::new(),
        };
        assert!(signed_message(&release).starts_with(b"releases-1\n"));
        let mut flagged = release.clone();
        flagged.security = true;
        assert_ne!(signed_message(&release), signed_message(&flagged));
    }
}
