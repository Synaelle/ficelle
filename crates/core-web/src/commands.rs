// SPDX-License-Identifier: AGPL-3.0-or-later

//! The apps' commands (`src-tauri/src/commands`), by the same names and
//! arguments, for what the web version keeps.

use std::path::Path;
use std::sync::Arc;

use app_core::crypto::KdfParams;
use app_core::id::{AttachmentId, FolderId, NoteId, TagId, VersionId};
use app_core::model::{NoteScope, NoteSort};
use app_core::store::{NewNote, NoteChange, Resolution, Store};
use app_core::vault::{Encryption, Vault};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::disk::WebDisk;
use crate::{CommandError, CommandResult, SESSION, Session};

/// Where the vault is in the browser's storage.
const VAULT: &str = "/vault";

/// A command's arguments, by the names the page sends.
pub struct Args(pub Value);

impl Args {
    /// One argument; a missing one reads as `null`, for the optional ones.
    pub fn get<T: DeserializeOwned>(&self, name: &str) -> CommandResult<T> {
        serde_json::from_value(self.0.get(name).cloned().unwrap_or(Value::Null))
            .map_err(|e| CommandError::new("invalid", format!("{name}: {e}")))
    }
}

/// Which notes to list, as the page sends it.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum Scope {
    All,
    Folder { id: Option<FolderId> },
    Tag { id: TagId },
    Untagged,
    Trash,
}

impl From<Scope> for NoteScope {
    fn from(scope: Scope) -> Self {
        match scope {
            Scope::All => Self::All,
            Scope::Folder { id } => Self::Folder(id),
            Scope::Tag { id } => Self::Tag(id),
            Scope::Untagged => Self::Untagged,
            Scope::Trash => Self::Trash,
        }
    }
}

/// How the vault opens: the web version has no device to keep a key, so a
/// password or nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Way {
    Plain,
    Device,
    Guarded,
    Pin,
    Password,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent facts the ui reads as plain json fields"
)]
struct VaultStatus {
    exists: bool,
    encrypted: bool,
    unlocked: bool,
    way: Way,
    has_password: bool,
    biometric_available: bool,
    device_available: bool,
    recovery: bool,
    pin_tries_left: Option<u8>,
}

/// An attachment with where its file stands, see `attachment_files`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AttachmentFile {
    #[serde(flatten)]
    attachment: app_core::model::Attachment,
    here: bool,
    refused: Option<String>,
}

fn disk() -> Arc<dyn app_core::disk::Disk> {
    Arc::new(WebDisk)
}

fn dir() -> &'static Path {
    Path::new(VAULT)
}

pub fn answer(value: impl Serialize) -> CommandResult<Value> {
    serde_json::to_value(value).map_err(|e| CommandError::new("invalid", e.to_string()))
}

/// Runs `f` with the unlocked session, or fails with `locked`.
pub fn with<T: Serialize>(
    f: impl FnOnce(&mut Session) -> app_core::Result<T>,
) -> CommandResult<Value> {
    let value = SESSION.with_borrow_mut(|session| {
        let session = session.as_mut().ok_or_else(CommandError::locked)?;
        Ok::<_, CommandError>(f(session)?)
    })?;
    answer(value)
}

/// Opens a session as the apps do: trash past its time goes, and notes
/// left blank.
fn open(vault: Vault, mut store: Store) -> CommandResult<Value> {
    store.purge_expired_trash()?;
    store.discard_blank_notes()?;
    let blobs = vault.blobs(&mut store)?;
    SESSION.with_borrow_mut(|session| {
        *session = Some(Session {
            vault,
            store,
            blobs,
        });
    });
    Ok(Value::Null)
}

fn needs_password() -> CommandError {
    CommandError::new("invalid", "a password is needed")
}

fn status() -> CommandResult<Value> {
    let unlocked = SESSION.with_borrow(Option::is_some);
    let disk = disk();
    if !Vault::exists_on(disk.as_ref(), dir()) {
        return answer(VaultStatus {
            exists: false,
            encrypted: false,
            unlocked,
            way: Way::Password,
            has_password: false,
            biometric_available: false,
            device_available: false,
            recovery: false,
            pin_tries_left: None,
        });
    }
    let vault = Vault::open_on(disk, dir())?;
    answer(VaultStatus {
        exists: true,
        encrypted: vault.is_encrypted(),
        unlocked,
        way: if vault.is_encrypted() {
            Way::Password
        } else {
            Way::Plain
        },
        has_password: vault.has_password(),
        biometric_available: false,
        device_available: false,
        recovery: vault.has_recovery_key(),
        pin_tries_left: None,
    })
}

fn create(a: &Args) -> CommandResult<Value> {
    let way: Way = a.get("way")?;
    let password: String = a.get("password")?;
    let encryption = match way {
        Way::Plain => Encryption::Off,
        Way::Password if !password.is_empty() => Encryption::On {
            password: &password,
            kdf: KdfParams::DEFAULT,
        },
        Way::Password => return Err(needs_password()),
        Way::Device | Way::Guarded | Way::Pin => return Err(CommandError::unsupported()),
    };
    let (vault, store) = Vault::create_on(disk(), dir(), encryption)?;
    open(vault, store)
}

fn unlock(a: &Args) -> CommandResult<Value> {
    let password: Option<String> = a.get("password")?;
    let vault = Vault::open_on(disk(), dir())?;
    let store = match (vault.is_encrypted(), password) {
        (false, _) => vault.unlock_plain()?,
        (true, Some(password)) => vault.unlock_with_password(&password)?,
        (true, None) => return Err(needs_password()),
    };
    open(vault, store)
}

/// Encrypts the open vault with a password. A failed try leaves the notes
/// open as they were.
fn encrypt(a: &Args) -> CommandResult<Value> {
    let password: Option<String> = a.get("password")?;
    let password = password
        .filter(|p| !p.is_empty())
        .ok_or_else(needs_password)?;
    let Some(session) = SESSION.with_borrow_mut(Option::take) else {
        return Err(CommandError::locked());
    };
    let mut vault = session.vault;
    drop(session.blobs);
    match vault.encrypt_with_password(session.store, &password, KdfParams::DEFAULT) {
        Ok(store) => open(vault, store),
        Err(e) => {
            let vault = Vault::open_on(disk(), dir())?;
            if !vault.is_encrypted() {
                let store = vault.unlock_plain()?;
                open(vault, store)?;
            }
            Err(e.into())
        }
    }
}

/// The key of the vault now: with its password, or for an unencrypted one
/// none is needed.
fn current_key(
    vault: &Vault,
    password: Option<&str>,
) -> CommandResult<app_core::crypto::DatabaseKey> {
    match password {
        Some(password) => Ok(vault.key_with_password(password)?),
        None => Err(needs_password()),
    }
}

/// Reads the vault again after its file changed, for the open session.
fn refresh() -> CommandResult<()> {
    let fresh = Vault::open_on(disk(), dir())?;
    SESSION.with_borrow_mut(|session| {
        if let Some(session) = session.as_mut() {
            session.vault = fresh;
        }
    });
    Ok(())
}

fn vault_command(name: &str, a: &Args) -> Option<CommandResult<Value>> {
    Some(match name {
        "vault_status" => status(),
        "vault_create" => create(a),
        "vault_unlock" => unlock(a),
        "vault_encrypt" => encrypt(a),
        "vault_lock" => {
            SESSION.with_borrow_mut(|session| *session = None);
            Ok(Value::Null)
        }
        "vault_reset" => {
            SESSION.with_borrow_mut(|session| *session = None);
            Vault::destroy_on(&WebDisk, dir())
                .map(|()| Value::Null)
                .map_err(Into::into)
        }
        "vault_change_password" => (|| {
            let old: String = a.get("old")?;
            let new: String = a.get("new")?;
            SESSION.with_borrow_mut(|session| {
                let session = session.as_mut().ok_or_else(CommandError::locked)?;
                session
                    .vault
                    .change_password(&old, &new, KdfParams::DEFAULT)?;
                Ok(Value::Null)
            })
        })(),
        "vault_check_password" => (|| {
            let password: String = a.get("password")?;
            let vault = Vault::open_on(disk(), dir())?;
            answer(!password.is_empty() && vault.key_with_password(&password).is_ok())
        })(),
        "vault_create_recovery" => (|| {
            let password: Option<String> = a.get("password")?;
            let mut vault = Vault::open_on(disk(), dir())?;
            let key = current_key(&vault, password.as_deref())?;
            let code = vault
                .create_recovery_key_for(&key, KdfParams::DEFAULT)?
                .to_string();
            refresh()?;
            answer(code)
        })(),
        "vault_remove_recovery" => (|| {
            let password: Option<String> = a.get("password")?;
            let mut vault = Vault::open_on(disk(), dir())?;
            let key = current_key(&vault, password.as_deref())?;
            vault.remove_recovery_key_for(&key)?;
            refresh()?;
            Ok(Value::Null)
        })(),
        "vault_recover" => (|| {
            let code: String = a.get("code")?;
            let new_password: String = a.get("newPassword")?;
            if new_password.is_empty() {
                return Err(needs_password());
            }
            let mut vault = Vault::open_on(disk(), dir())?;
            let key = vault.key_with_recovery(&code)?;
            vault.set_password(&key, &new_password, KdfParams::DEFAULT)?;
            let store = vault.unlock_with_key(&key)?;
            open(vault, store)
        })(),
        _ => return None,
    })
}

fn note_command(name: &str, a: &Args) -> Option<CommandResult<Value>> {
    let id = || a.get::<NoteId>("id");
    Some(match name {
        "list_notes" => (|| {
            let scope: Scope = a.get("scope")?;
            let sort: NoteSort = a.get("sort")?;
            with(|s| s.store.list_notes(scope.into(), sort))
        })(),
        "get_note" => id().and_then(|id| with(|s| s.store.note(id))),
        "create_note" => a
            .get::<NewNote>("note")
            .and_then(|note| with(|s| s.store.create_note(&note))),
        "update_note" => (|| {
            let change: NoteChange = a.get("change")?;
            let id = id()?;
            with(|s| s.store.update_note(id, change))
        })(),
        "set_pinned" => (|| {
            let (id, pinned): (NoteId, bool) = (id()?, a.get("pinned")?);
            with(|s| s.store.set_pinned(id, pinned))
        })(),
        "move_note" => (|| {
            let (id, folder): (NoteId, Option<FolderId>) = (id()?, a.get("folder")?);
            with(|s| s.store.move_note(id, folder))
        })(),
        "set_note_tags" => (|| {
            let (id, tags): (NoteId, Vec<String>) = (id()?, a.get("tags")?);
            with(|s| s.store.set_note_tags(id, &tags))
        })(),
        "trash_note" => id().and_then(|id| with(|s| s.store.trash_note(id))),
        "restore_note" => id().and_then(|id| with(|s| s.store.restore_note(id))),
        "purge_note" => id().and_then(|id| with(|s| s.store.purge_note(id))),
        "discard_if_blank" => id().and_then(|id| with(|s| s.store.discard_if_blank(id))),
        "empty_trash" => with(|s| {
            let now = app_core::clock::Clock::now(&app_core::clock::SystemClock);
            s.store.purge_trashed_before(now)
        }),
        "list_versions" => id().and_then(|id| with(|s| s.store.list_versions(id))),
        "restore_version" => a
            .get::<VersionId>("id")
            .and_then(|id| with(|s| s.store.restore_version(id))),
        "note_conflict" => id().and_then(|id| with(|s| s.store.note_conflict(id))),
        "conflicted_notes" => with(|s| s.store.conflicted_notes()),
        "resolve_conflict" => (|| {
            let (id, keep): (NoteId, Resolution) = (id()?, a.get("keep")?);
            with(|s| s.store.resolve_conflict(id, &keep))
        })(),
        "search" => (|| {
            let (query, limit): (String, usize) = (a.get("query")?, a.get("limit")?);
            with(|s| s.store.search(&query, limit))
        })(),
        "backlinks" => id().and_then(|id| with(|s| s.store.backlinks(id))),
        "resolve_link" => a
            .get::<String>("target")
            .and_then(|target| with(|s| s.store.resolve_link(&target))),
        "link_suggestions" => (|| {
            let (input, limit): (String, usize) = (a.get("input")?, a.get("limit")?);
            with(|s| s.store.link_suggestions(&input, limit))
        })(),
        "note_headings" => id().and_then(|id| with(|s| s.store.note_headings(id))),
        _ => return None,
    })
}

fn folder_and_tag_command(name: &str, a: &Args) -> Option<CommandResult<Value>> {
    let folder = || a.get::<FolderId>("id");
    let tag = || a.get::<TagId>("id");
    Some(match name {
        "list_folders" => with(|s| s.store.list_folders()),
        "create_folder" => (|| {
            let (parent, name): (Option<FolderId>, String) = (a.get("parent")?, a.get("name")?);
            with(|s| s.store.create_folder(parent, &name))
        })(),
        "rename_folder" => (|| {
            let (id, name): (FolderId, String) = (folder()?, a.get("name")?);
            with(|s| s.store.rename_folder(id, &name))
        })(),
        "move_folder" => (|| {
            let (id, parent): (FolderId, Option<FolderId>) = (folder()?, a.get("parent")?);
            with(|s| s.store.move_folder(id, parent))
        })(),
        "trash_folder" => folder().and_then(|id| with(|s| s.store.trash_folder(id))),
        "list_trashed_folders" => with(|s| s.store.list_trashed_folders()),
        "restore_folder" => folder().and_then(|id| with(|s| s.store.restore_folder(id))),
        "list_tags" => with(|s| s.store.list_tags()),
        "create_tag" => a
            .get::<String>("name")
            .and_then(|name| with(|s| s.store.create_tag(&name))),
        "rename_tag" => (|| {
            let (id, name): (TagId, String) = (tag()?, a.get("name")?);
            with(|s| s.store.rename_tag(id, &name))
        })(),
        "set_tag_color" => (|| {
            let (id, color): (TagId, Option<String>) = (tag()?, a.get("color")?);
            with(|s| s.store.set_tag_color(id, color.as_deref()))
        })(),
        "delete_tag" => tag().and_then(|id| with(|s| s.store.delete_tag(id))),
        "synced_settings" => with(|s| s.store.synced_settings()),
        "set_synced_setting" => (|| {
            let (key, value): (String, String) = (a.get("key")?, a.get("value")?);
            with(|s| s.store.set_synced_setting(&key, &value))
        })(),
        "tags_merged" => with(|s| s.store.tags_merged()),
        "tags_merged_seen" => with(|s| s.store.tags_merged_seen()),
        _ => return None,
    })
}

/// The largest file the page hands over, as in the apps.
const MAX_ATTACHMENT_BYTES: usize = 1024 * 1024 * 1024;

fn file_command(name: &str, a: &Args, bytes: Option<Vec<u8>>) -> Option<CommandResult<Value>> {
    let note = || a.get::<NoteId>("note");
    Some(match name {
        "add_attachment" => (|| {
            let bytes = bytes.ok_or_else(|| CommandError::new("invalid", "no file"))?;
            if bytes.len() > MAX_ATTACHMENT_BYTES {
                return Err(CommandError::new("tooLarge", "this file is too large"));
            }
            let (note, name, mime): (NoteId, String, String) =
                (note()?, a.get("name")?, a.get("mime")?);
            let size = i64::try_from(bytes.len()).unwrap_or(i64::MAX);
            with(|s| {
                let sha = s.blobs.put(&bytes)?;
                s.store.add_attachment(note, &name, &mime, size, &sha)
            })
        })(),
        "set_attachment_thumb" => (|| {
            let bytes = bytes.ok_or_else(|| CommandError::new("invalid", "no picture"))?;
            let id: AttachmentId = a.get("id")?;
            with(|s| {
                let sha = s.blobs.put(&bytes)?;
                s.store.set_attachment_thumb(id, &sha)
            })
        })(),
        "list_attachments" => a
            .get::<NoteId>("id")
            .and_then(|id| with(|s| s.store.list_attachments(id))),
        "all_attachments" => with(|s| s.store.all_attachments()),
        "remove_attachment" => a
            .get::<AttachmentId>("id")
            .and_then(|id| with(|s| s.store.remove_attachment(id))),
        "attachment_files" => note().and_then(|note| {
            with(|s| {
                s.store
                    .list_attachments(note)?
                    .into_iter()
                    .map(|attachment| {
                        Ok(AttachmentFile {
                            here: s.blobs.has(&attachment.sha256)?,
                            refused: None,
                            attachment,
                        })
                    })
                    .collect::<app_core::Result<Vec<_>>>()
            })
        }),
        // what the apps learn from a server: there is none here
        "files_info" => Ok(Value::Null),
        _ => return None,
    })
}

/// Runs a command by name.
///
/// # Errors
///
/// The command's own, `unsupported` for one the web version does not have.
pub fn run(name: &str, a: &Args, bytes: Option<Vec<u8>>) -> CommandResult<Value> {
    if let Some(done) = vault_command(name, a)
        .or_else(|| note_command(name, a))
        .or_else(|| folder_and_tag_command(name, a))
    {
        return done;
    }
    match name {
        "add_attachment" | "set_attachment_thumb" => file_command(name, a, bytes),
        _ => file_command(name, a, None).or_else(|| crate::files::run(name, a, bytes)),
    }
    .unwrap_or_else(|| Err(CommandError::unsupported()))
}

/// Runs a command that answers with bytes.
///
/// # Errors
///
/// As [`run`].
pub fn bytes(name: &str, a: &Args) -> CommandResult<Vec<u8>> {
    match name {
        "read_attachment" => {
            let sha256: String = a.get("sha256")?;
            SESSION.with_borrow(|session| {
                let session = session.as_ref().ok_or_else(CommandError::locked)?;
                session
                    .blobs
                    .get(&sha256)?
                    .ok_or_else(|| CommandError::new("notFound", "the file is not on this device"))
            })
        }
        _ => crate::files::bytes(name, a).unwrap_or_else(|| Err(CommandError::unsupported())),
    }
}
