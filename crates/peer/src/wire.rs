// SPDX-License-Identifier: AGPL-3.0-or-later

//! What two devices say inside the channel. A message is one kind byte,
//! then JSON for [`Msg`], or a file's hash and bytes for a file, which are
//! too large to carry as text.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};

use crate::Error;

const JSON: u8 = 0;
const FILE: u8 = 1;

/// A document's version or changes, the bytes as base64.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Doc {
    pub id: String,
    pub bytes: String,
}

impl Doc {
    pub fn new(id: String, bytes: &[u8]) -> Self {
        Self {
            id,
            bytes: STANDARD.encode(bytes),
        }
    }
}

/// Docs back to ids and bytes.
pub(crate) fn decode_docs(docs: Vec<Doc>) -> Result<Vec<(String, Vec<u8>)>, Error> {
    docs.into_iter()
        .map(|d| {
            STANDARD
                .decode(d.bytes)
                .map(|b| (d.id, b))
                .map_err(|_| Error::Protocol("bytes that are not base64"))
        })
        .collect()
}

/// A device in a device list.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Device {
    pub key: String,
    pub name: String,
    pub platform: String,
    pub paired_at: i64,
    pub forgotten_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "camelCase")]
pub(crate) enum Msg {
    /// First message each way: who this is and what it speaks.
    #[serde(rename_all = "camelCase")]
    Hello {
        name: String,
        platform: String,
        version: String,
        min_protocol: u32,
        protocol: u32,
        format: u32,
        /// The port the sender listens on, so a device it dialled can dial
        /// it back at that address later.
        #[serde(default)]
        port: Option<u16>,
    },
    /// Every device the sender knows, forgotten ones too.
    Devices {
        devices: Vec<Device>,
    },
    /// The sender's version of each document. `answer`: send yours back.
    Versions {
        docs: Vec<Doc>,
        answer: bool,
    },
    /// Changes the receiver lacked, in batches, `last` on the final one.
    Changes {
        docs: Vec<Doc>,
        last: bool,
    },
    /// Files the sender lacks, by hash.
    Want {
        files: Vec<String>,
    },
    /// A file asked for that the sender does not have either.
    Missing {
        sha256: String,
    },
    Ping,
    Pong,
    /// The sender closes, and why.
    Bye {
        reason: String,
    },
}

/// What came in.
pub(crate) enum Incoming {
    Msg(Msg),
    File { sha256: String, bytes: Vec<u8> },
}

pub(crate) fn encode(msg: &Msg) -> Vec<u8> {
    let mut out = vec![JSON];
    // serializing these plain types cannot fail
    out.extend(serde_json::to_vec(msg).unwrap_or_default());
    out
}

pub(crate) fn encode_file(sha256: &str, bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 65);
    out.push(FILE);
    out.extend_from_slice(sha256.as_bytes());
    out.extend_from_slice(bytes);
    out
}

pub(crate) fn decode(message: &[u8]) -> Result<Incoming, Error> {
    match message.split_first() {
        Some((&JSON, json)) => serde_json::from_slice(json)
            .map(Incoming::Msg)
            .map_err(|_| Error::Protocol("a message that does not read")),
        Some((&FILE, rest)) if rest.len() >= 64 => {
            let (sha, bytes) = rest.split_at(64);
            let sha256 = std::str::from_utf8(sha)
                .map_err(|_| Error::Protocol("a file hash that does not read"))?
                .to_owned();
            Ok(Incoming::File {
                sha256,
                bytes: bytes.to_vec(),
            })
        }
        _ => Err(Error::Protocol("an unknown message")),
    }
}
