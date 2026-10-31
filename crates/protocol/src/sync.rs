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

/// A role in a shared space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SpaceRole {
    /// Shared it: edits, adds and removes people, stops sharing.
    Owner,
    /// Reads and edits.
    Edit,
    /// Reads only.
    Read,
}

/// A key of an e2ee shared space, sealed for the account reading it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceGrant {
    /// The key's epoch: a new one starts when someone is removed.
    pub epoch: u32,
    /// The space key sealed with HPKE to this account's public key.
    pub grant: String,
}

/// A member of an e2ee space who has no key for its latest epoch yet: the
/// owner's devices seal one for them, the server cannot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantWanted {
    /// The member.
    pub user_id: String,
    /// Their published public key.
    pub public_key: String,
    /// The epoch they have no key for. A member added after a removal
    /// needs the older epochs too, to read what was written before.
    pub epoch: u32,
    /// Their username, for the owner to recognise someone waiting to be let
    /// in. Empty from servers before 0.2.1.
    #[serde(default)]
    pub name: String,
}

/// A shared space this account belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedSpace {
    /// Its id.
    pub id: String,
    /// This account's role.
    pub role: SpaceRole,
    /// Standard or e2ee, the owner's kind.
    pub kind: AccountKind,
    /// The owner's username, shown as "shared by".
    pub owner: String,
    /// E2ee: the owner's public key, which members' devices keep from the
    /// first time they see it. Grants are sealed by the owner's key pair
    /// and only taken from that key. Empty from servers before 0.3.0.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub owner_key: String,
    /// Number of its latest change.
    pub head: i64,
    /// E2ee: the latest key epoch.
    #[serde(default)]
    pub epoch: u32,
    /// E2ee: this account's keys for it, every epoch it was a member in.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grants: Vec<SpaceGrant>,
    /// E2ee, owner only: members waiting for a key.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grants_wanted: Vec<GrantWanted>,
}

/// A key sealed by an owner's device for one member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantGiven {
    /// The member.
    pub user_id: String,
    /// The space key sealed to their public key.
    pub grant: String,
}

/// `POST /api/v1/sync/spaces/{id}/grants`: keys for members of an e2ee
/// space, from one of the owner's devices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GiveGrants {
    /// The epoch the key is for.
    pub epoch: u32,
    /// One sealed key per member.
    pub grants: Vec<GrantGiven>,
}

/// `GET /api/v1/sync/spaces`: the shared spaces this account belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedSpaces {
    /// The spaces, owned ones included.
    pub spaces: Vec<SharedSpace>,
}

/// Someone or a group a space is shared with, as the owner chooses them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberChoice {
    /// An account, or
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    /// a group: everyone in it, as it changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    /// `edit` or `read`.
    pub role: SpaceRole,
}

/// `POST /api/v1/sync/spaces` makes a shared space, `PUT
/// /api/v1/sync/spaces/{id}/members` sets who is in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareRequest {
    /// Everyone it is shared with, the owner aside.
    pub members: Vec<MemberChoice>,
}

/// A member of a shared space.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceMember {
    /// The account, for a person.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    /// The group, for a group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    /// Username, or the group's name.
    pub name: String,
    /// Shown name of a person.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Their role.
    pub role: SpaceRole,
    /// E2ee: a person's public key, to compare fingerprints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_key: Option<String>,
}

/// `GET /api/v1/sync/spaces/{id}/members`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceMembers {
    /// The owner first, then people and groups by name.
    pub members: Vec<SpaceMember>,
}

/// `GET /api/v1/sync/people?name=`: an account to share with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    /// Account id.
    pub user_id: String,
    /// Username.
    pub username: String,
    /// Shown name.
    pub display_name: String,
    /// Standard or e2ee.
    pub kind: AccountKind,
    /// E2ee: their public key, absent until a device of theirs made one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_key: Option<String>,
}

/// `GET` and `PUT /api/v1/sync/key-pair`: an e2ee account's key pair for
/// sharing, made by its first device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountKeyPair {
    /// The public key, everyone sharing with the account seals to it.
    pub public_key: String,
    /// The private key sealed by the account's master key.
    pub private_sealed: String,
}

/// `GET /api/v1/sync/live`, a WebSocket: what the server tells a device.
/// The device sends pings and [`LiveRequest`]s.
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
    /// Another device pushed changes, or the shared spaces changed: pull.
    Changed {
        /// Number of the latest change now, 0 when unknown.
        head: i64,
        /// The shared space that changed, absent for the account's own.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        space: Option<String>,
    },
    /// The admin changed the co-editing setting.
    Settings {
        /// See [`SpaceInfo::coediting`].
        coediting: bool,
    },
    /// Who has a note of a shared space open, sent to everyone in it when
    /// that changes.
    Presence {
        /// The shared space.
        space: String,
        /// Everyone viewing one of its notes, this account's devices
        /// included.
        viewers: Vec<Viewer>,
    },
}

/// Someone with a note of a shared space open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Viewer {
    /// Their username.
    pub name: String,
    /// Their shown name.
    pub display_name: String,
    /// The note they have open.
    pub note: String,
}

/// What a device says on its live socket, besides pings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum LiveRequest {
    /// The note open on this device, if it is in a shared space: the
    /// others in the space see it. Both absent when none is.
    Viewing {
        /// The shared space.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        space: Option<String>,
        /// The note.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
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
        assert_eq!(
            changed,
            LiveMessage::Changed {
                head: 9,
                space: None
            }
        );
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
