// SPDX-License-Identifier: AGPL-3.0-or-later

//! Sharing from the apps: what is shared here, sharing a folder or a note,
//! who it is shared with, stopping or leaving. The server keeps who is in a
//! space, the vault keeps its notes, the sync thread does the rest (keys
//! for an e2ee space included). Also the account's key pair, made by the
//! first device of an e2ee account at its first round.

use app_core::id::{FolderId, NoteId};
use app_core::store::{Item, LocalSpace, SpaceRoots, Tokens, WaitingMember};
use app_core::sync::engine::{RemoteError, SyncError};
use app_protocol::admin::Group;
use app_protocol::sync::{
    AccountKeyPair, MemberChoice, Person, ShareRequest, SharedSpace, SharedSpaces, SpaceMember,
    SpaceMembers,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use super::http::{Api, HttpError};
use super::sign_in::SignInError;
use super::{Vault, Wire, emit_status};
use crate::state::AppState;

/// Makes sure this e2ee device holds the account's key pair: the one the
/// server keeps, opened with the master key, or a new one sent when the
/// account has none. A server from before sharing has nowhere to keep it,
/// sharing is then simply not there.
pub(super) fn ensure_key_pair(state: &AppState, wire: &mut Wire<'_>) -> Result<(), SyncError> {
    use app_core::sync::engine::Access;
    if Vault(state).with(|s| s.sync_key_pair())?.is_some() {
        return Ok(());
    }
    let Some(master) = Vault(state).with(|s| s.sync_master_key())? else {
        return Ok(());
    };
    let bad = |e: HttpError| RemoteError::Refused(e.to_string());
    let kept: AccountKeyPair = match wire.call(|api, token| api.get("/sync/key-pair", Some(token)))
    {
        Ok(answer) => answer.parse().map_err(bad)?,
        Err(RemoteError::Refused(_)) => {
            let pair = app_e2ee::new_key_pair();
            let made = AccountKeyPair {
                public_key: pair.public_text(),
                private_sealed: app_e2ee::seal_key_pair(&master, &pair),
            };
            match wire.call(|api, token| api.put("/sync/key-pair", Some(token), &made)) {
                // another device may have sent one first: the answer is the one kept
                Ok(answer) => answer.parse().map_err(bad)?,
                Err(RemoteError::Refused(_)) => return Ok(()),
                Err(other) => return Err(other.into()),
            }
        }
        Err(other) => return Err(other.into()),
    };
    let pair = app_e2ee::open_key_pair(&master, &kept.public_key, &kept.private_sealed)
        .map_err(|_| app_core::Error::Corrupt("the account's key pair".into()))?;
    Vault(state).with(|s| s.sync_set_key_pair(&pair))?;
    Ok(())
}

pub(super) fn failed(code: &str, message: impl Into<String>) -> SignInError {
    SignInError {
        code: code.into(),
        message: message.into(),
        retry_after_seconds: None,
    }
}

fn vault_error(err: impl std::fmt::Debug) -> SignInError {
    failed("vault", format!("{err:?}"))
}

/// Runs a call to the server with this device's tokens, refreshed once
/// when they ran out.
pub(super) fn authed<T>(
    state: &AppState,
    call: impl Fn(&Api, &str) -> Result<T, HttpError>,
) -> Result<T, SignInError> {
    let link = state
        .quietly(|s| s.store.sync_link())
        .map_err(vault_error)?
        .ok_or_else(|| failed("signedOut", "not signed in"))?;
    let tokens: Tokens = link
        .tokens
        .ok_or_else(|| failed("signedOut", "sign in again"))?;
    let api = Api::new(&link.server);
    match call(&api, &tokens.access) {
        Err(HttpError::Api { status: 401, .. }) => {
            let fresh = super::refresh(state, &api, &tokens.access).map_err(|e| match e {
                RemoteError::Offline(why) => failed("offline", why),
                RemoteError::SignedOut => failed("signedOut", "sign in again"),
                RemoteError::Refused(why) => failed("refused", why),
            })?;
            Ok(call(&api, &fresh.access)?)
        }
        other => Ok(other?),
    }
}

pub(super) async fn blocking<T: Send + 'static>(
    app: AppHandle,
    f: impl FnOnce(&AppState) -> Result<T, SignInError> + Send + 'static,
) -> Result<T, SignInError> {
    tauri::async_runtime::spawn_blocking(move || f(&app.state::<AppState>()))
        .await
        .map_err(|e| failed("internal", e.to_string()))?
}

/// Takes in the server's list of shared spaces: joined, updated, dropped.
fn refresh_spaces(state: &AppState) -> Result<(), SignInError> {
    let listed: SharedSpaces = authed(state, |api, token| {
        api.get("/sync/spaces", Some(token))?.parse()
    })?;
    state
        .with(|s| s.store.sync_set_spaces(&listed.spaces))
        .map_err(vault_error)?;
    Ok(())
}

/// A shared space as the sidebar shows it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedEntry {
    #[serde(flatten)]
    space: LocalSpace,
    #[serde(flatten)]
    roots: SpaceRoots,
}

/// The shared spaces in this vault and what each holds at its top.
#[tauri::command]
pub async fn share_list(state: State<'_, AppState>) -> Result<Vec<SharedEntry>, SignInError> {
    state
        .quietly(|s| {
            s.store
                .shared_spaces()?
                .into_iter()
                .map(|space| {
                    let roots = s.store.space_roots(&space.id)?;
                    Ok(SharedEntry { space, roots })
                })
                .collect()
        })
        .map_err(vault_error)
}

/// Someone to share with, and the fingerprint of their key to compare.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonView {
    #[serde(flatten)]
    person: Person,
    fingerprint: Option<String>,
}

fn fingerprint(public_key: Option<&str>) -> Option<String> {
    public_key.and_then(|k| app_e2ee::fingerprint(k).ok())
}

#[tauri::command]
pub async fn share_find(app: AppHandle, name: String) -> Result<PersonView, SignInError> {
    blocking(app, move |state| {
        let path = format!("/sync/people?name={}", urlencode(&name));
        let person: Person = authed(state, |api, token| api.get(&path, Some(token))?.parse())?;
        Ok(PersonView {
            fingerprint: fingerprint(person.public_key.as_deref()),
            person,
        })
    })
    .await
}

fn urlencode(text: &str) -> String {
    text.trim()
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.') {
                char::from(b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// The groups this account is in, to share with.
#[tauri::command]
pub async fn share_groups(app: AppHandle) -> Result<Vec<Group>, SignInError> {
    blocking(app, |state| {
        authed(state, |api, token| {
            api.get("/me/groups", Some(token))?.parse()
        })
    })
    .await
}

/// A member, with the fingerprint of a person's key.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberView {
    #[serde(flatten)]
    member: SpaceMember,
    fingerprint: Option<String>,
}

fn views(members: SpaceMembers) -> Vec<MemberView> {
    members
        .members
        .into_iter()
        .map(|member| MemberView {
            fingerprint: fingerprint(member.public_key.as_deref()),
            member,
        })
        .collect()
}

#[tauri::command]
pub async fn share_members(app: AppHandle, space: String) -> Result<Vec<MemberView>, SignInError> {
    blocking(app, move |state| {
        let path = format!("/sync/spaces/{space}/members");
        let members: SpaceMembers =
            authed(state, |api, token| api.get(&path, Some(token))?.parse())?;
        Ok(views(members))
    })
    .await
}

/// The people the owner chose here are let in with the key the server shows
/// for them now, or the first one they will have: their space keys are
/// sealed from this device. Someone the server adds otherwise (through a
/// group, from another device, a changed key) waits for the owner, see
/// `share_waiting`. Somebody let in already keeps the key they had.
fn trust_chosen(state: &AppState, space: &str, chosen: &[MemberChoice], members: &SpaceMembers) {
    let _ = state.with(|s| {
        for user_id in chosen.iter().filter_map(|c| c.user_id.as_deref()) {
            if s.store.space_trusted(space, user_id)? {
                continue;
            }
            let key = members
                .members
                .iter()
                .find(|m| m.user_id.as_deref() == Some(user_id))
                .and_then(|m| m.public_key.as_deref());
            s.store.space_trust(space, user_id, key)?;
        }
        Ok(())
    });
}

/// Someone waiting to be let into a space, with their key's fingerprint.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitingView {
    #[serde(flatten)]
    member: WaitingMember,
    fingerprint: Option<String>,
}

/// Members of a space this account owns that the server lists as waiting
/// for its keys, and the owner has not let in from this device.
#[tauri::command]
pub async fn share_waiting(app: AppHandle, space: String) -> Result<Vec<WaitingView>, SignInError> {
    blocking(app, move |state| {
        let waiting = state
            .with(|s| s.store.space_waiting(&space))
            .map_err(vault_error)?;
        Ok(waiting
            .into_iter()
            .map(|member| WaitingView {
                fingerprint: fingerprint(Some(&member.public_key)),
                member,
            })
            .collect())
    })
    .await
}

/// Lets a waiting member in with the key the owner compared: the keys are
/// sealed for it at the next round, never for another key the server
/// shows later.
#[tauri::command]
pub async fn share_allow(
    app: AppHandle,
    space: String,
    user_id: String,
    public_key: String,
) -> Result<(), SignInError> {
    blocking(app, move |state| {
        if !app_e2ee::public_key_ok(&public_key) {
            return Err(failed("invalid", "not a public key"));
        }
        state
            .with(|s| s.store.space_trust(&space, &user_id, Some(&public_key)))
            .map_err(vault_error)?;
        state.sync.now();
        Ok(())
    })
    .await
}

/// What to share.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ShareItem {
    Note { id: NoteId },
    Folder { id: FolderId },
}

/// Shares a folder or a note: a new shared space on the server, then the
/// item moves into it here. Returns the item's id in the space.
#[tauri::command]
pub async fn share_create(
    app: AppHandle,
    item: ShareItem,
    members: Vec<MemberChoice>,
) -> Result<String, SignInError> {
    let emit = app.clone();
    let id = blocking(app, move |state| {
        let request = ShareRequest { members };
        let space: SharedSpace = authed(state, |api, token| {
            api.post("/sync/spaces", Some(token), &request)?.parse()
        })?;
        let path = format!("/sync/spaces/{}/members", space.id);
        let listed: SpaceMembers =
            authed(state, |api, token| api.get(&path, Some(token))?.parse())?;
        trust_chosen(state, &space.id, &request.members, &listed);
        refresh_spaces(state)?;
        let item = match item {
            ShareItem::Note { id } => Item::Note(id),
            ShareItem::Folder { id } => Item::Folder(id),
        };
        let moved = state
            .with(|s| s.store.move_to_space(item, &space.id))
            .map_err(vault_error)?;
        state.sync.now();
        Ok(moved)
    })
    .await?;
    emit_status(&emit);
    Ok(id)
}

#[tauri::command]
pub async fn share_set_members(
    app: AppHandle,
    space: String,
    members: Vec<MemberChoice>,
) -> Result<Vec<MemberView>, SignInError> {
    blocking(app, move |state| {
        let path = format!("/sync/spaces/{space}/members");
        let request = ShareRequest { members };
        let members: SpaceMembers = authed(state, |api, token| {
            api.put(&path, Some(token), &request)?.parse()
        })?;
        trust_chosen(state, &space, &request.members, &members);
        // an owner's device seals keys for new members at its next round
        state.sync.now();
        Ok(views(members))
    })
    .await
}

/// Stops sharing: what the space holds moves back into the owner's own
/// notes first, then the server forgets the space and members lose it.
/// Offline, the notes are back already and stopping can be tried again.
#[tauri::command]
pub async fn share_stop(app: AppHandle, space: String) -> Result<(), SignInError> {
    blocking(app, move |state| {
        state
            .with(|s| {
                let owned = s
                    .store
                    .shared_spaces()?
                    .iter()
                    .any(|l| l.id == space && l.role == "owner");
                if !owned {
                    return Err(app_core::Error::Invalid(
                        "only the owner stops sharing".into(),
                    ));
                }
                let roots = s.store.space_roots(&space)?;
                for folder in roots.folders {
                    s.store.move_to_space(Item::Folder(folder), "")?;
                }
                for note in roots.notes {
                    s.store.move_to_space(Item::Note(note.id), "")?;
                }
                Ok(())
            })
            .map_err(vault_error)?;
        let path = format!("/sync/spaces/{space}");
        authed(state, |api, token| api.delete(&path, Some(token)))?;
        refresh_spaces(state)?;
        state.sync.now();
        Ok(())
    })
    .await
}

/// Leaves a space shared with this account: it goes from every device of
/// the account, the owner and the others keep it.
#[tauri::command]
pub async fn share_leave(app: AppHandle, space: String) -> Result<(), SignInError> {
    blocking(app, move |state| {
        let path = format!("/sync/spaces/{space}/me");
        authed(state, |api, token| api.delete(&path, Some(token)))?;
        refresh_spaces(state)?;
        Ok(())
    })
    .await
}
