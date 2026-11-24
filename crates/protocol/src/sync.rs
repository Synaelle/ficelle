// SPDX-License-Identifier: AGPL-3.0-or-later

//! Sync. A space is an ordered log of document changes (Loro updates). A
//! device pushes its own changes and pulls everyone's from where it left
//! off, in pages. The server numbers changes in the order it takes them,
//! that number is the device's cursor. Changes travel as base64 text: for
//! a standard account the bytes are the Loro update (encrypted at rest by
//! the server), for an e2ee account they are already encrypted by the
//! device and the server cannot read them.

use serde::{Deserialize, Serialize};

use crate::e2ee::AccountKind;

/// `GET /api/v1/sync`: the account's space.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceInfo {
    /// Id of the space.
    pub space_id: String,
    /// Standard or e2ee, decides who encrypts.
    pub kind: AccountKind,
    /// Number of the latest change, 0 when empty.
    pub head: i64,
    /// The admin allows co-editing: an open note follows other devices'
    /// edits as they are made.
    #[serde(default = "yes")]
    pub coediting: bool,
}

fn yes() -> bool {
    true
}

/// `GET /api/v1/sync/live`, a WebSocket: what the server tells a device.
/// The device sends only pings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum LiveMessage {
    /// First message: where the space stands and the admin's choice.
    #[serde(rename_all = "camelCase")]
    Hello {
        /// Number of the latest change.
        head: i64,
        /// See [`SpaceInfo::coediting`].
        coediting: bool,
    },
    /// Another device pushed changes: pull.
    Changed {
        /// Number of the latest change now, 0 when unknown.
        head: i64,
    },
    /// The admin changed the co-editing setting.
    Settings {
        /// See [`SpaceInfo::coediting`].
        coediting: bool,
    },
}

/// One change sent by a device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutgoingChange {
    /// The device's own id for the change, unique and stable across resends:
    /// a change sent twice is kept once.
    pub id: String,
    /// The document: `space` or a note id.
    pub doc: String,
    /// The change, base64.
    pub data: String,
    /// E2ee: the document's key wrapped by the account key, sent with the
    /// first change of a new document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// This change holds all of its document up to this number: the server
    /// empties the changes of the document it replaces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compacts: Option<i64>,
}

/// `GET /api/v1/sync/heavy?min=`: documents with many changes, worth a
/// compaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeavyDocs {
    /// The documents, most changes first.
    pub docs: Vec<HeavyDoc>,
}

/// One document of [`HeavyDocs`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeavyDoc {
    /// The document.
    pub doc: String,
    /// Its changes not compacted yet.
    pub changes: i64,
}

/// `POST /api/v1/sync/push`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushRequest {
    /// Changes, in the order they were made.
    pub changes: Vec<OutgoingChange>,
}

/// Answer of [`PushRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushAnswer {
    /// Changes newly kept.
    pub accepted: u32,
    /// Changes already kept from an earlier send.
    pub duplicates: u32,
    /// Number of the latest change now.
    pub head: i64,
}

/// One change as read back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncomingChange {
    /// Its number in the space, the next cursor.
    pub seq: i64,
    /// The sending device's name for it.
    #[serde(default)]
    pub id: String,
    /// The document.
    pub doc: String,
    /// The change, base64.
    pub data: String,
    /// The device that sent it, absent once that device is gone. A device
    /// can skip its own changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
}

/// A document's key, e2ee only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocKey {
    /// The document.
    pub doc: String,
    /// Its key wrapped by the account key.
    pub wrapped: String,
}

/// `GET /api/v1/sync/changes?after=&limit=`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullAnswer {
    /// Changes after the cursor, oldest first.
    pub changes: Vec<IncomingChange>,
    /// E2ee: keys of the documents in this page.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<DocKey>,
    /// Number of the latest change.
    pub head: i64,
    /// More changes wait after this page.
    pub more: bool,
    /// Name of the change numbered `after`, absent when there is none. A
    /// device that took a different change under that number knows the
    /// server's log is not the one it followed (restored from a backup,
    /// started over) and sends all it holds again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_messages_on_the_wire() {
        let hello = LiveMessage::Hello {
            head: 4,
            coediting: true,
        };
        assert_eq!(
            serde_json::to_string(&hello).unwrap(),
            r#"{"type":"hello","head":4,"coediting":true}"#
        );
        let changed: LiveMessage = serde_json::from_str(r#"{"type":"changed","head":9}"#).unwrap();
        assert_eq!(changed, LiveMessage::Changed { head: 9 });
        let from_a_sharing_server: LiveMessage =
            serde_json::from_str(r#"{"type":"changed","head":9,"space":"s1"}"#).unwrap();
        assert_eq!(from_a_sharing_server, LiveMessage::Changed { head: 9 });
        let old: SpaceInfo =
            serde_json::from_str(r#"{"spaceId":"s","kind":"standard","head":0}"#).unwrap();
        assert!(old.coediting, "a server from before the setting allows it");
    }

    #[test]
    fn changes_on_the_wire() {
        let change: OutgoingChange =
            serde_json::from_str(r#"{"id":"c1","doc":"space","data":"AAE"}"#).unwrap();
        assert_eq!(change.key, None);
        let answer = PullAnswer {
            changes: vec![],
            keys: vec![],
            head: 3,
            more: false,
            anchor: None,
        };
        let json = serde_json::to_value(&answer).unwrap();
        assert!(json.get("keys").is_none());
        assert_eq!(json["head"], 3);
    }
}
