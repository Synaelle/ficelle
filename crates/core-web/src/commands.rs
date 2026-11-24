// SPDX-License-Identifier: AGPL-3.0-or-later

//! The apps' commands (`src-tauri/src/commands`), by the same names and
//! arguments, for what the web version keeps.

use std::path::Path;
use std::sync::Arc;

use app_core::blobs::BlobStore;
use app_core::commands::vault::{self, Keys, VaultStatus, Way};
use app_core::crypto::KdfParams;
use app_core::id::{AttachmentId, FolderId, NoteId, TagId, VersionId};
use app_core::model::{NoteScope, NoteSort};
use app_core::store::{NewNote, NoteChange, Resolution, Store};
use app_core::vault::Vault;
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

/// Deletes the files no note refers to any more, those of removed
/// attachments too: the apps keep them a while for the other devices, the
/// web version has none, and the browser gives it little room.
fn clean_files(store: &mut Store, blobs: &BlobStore) -> app_core::Result<()> {
    let mut kept = store.kept_blobs()?;
    kept.extend(crate::files::put_for_imports());
    // the room of files removed before comes back too, once there is
    // enough for it, see reclaim
    let removed = blobs.remove_unreferenced(&kept)?;
    crate::disk::reclaim()?;
    // -1: removed in this very millisecond too
    if removed > 0 {
        match store.forget_removed_files(-1) {
            // forgotten at a later cleanup, once there is room for this write
            Err(err) if refused(&err) => {}
            forgotten => {
                forgotten?;
            }
        }
    }
    Ok(())
}

/// The browser's storage refused a write, as it does when full.
fn refused(err: &app_core::Error) -> bool {
    let db = match err {
        app_core::Error::Db(db) => Some(db),
        app_core::Error::Io(io) => io
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<rusqlite::Error>()),
        _ => None,
    };
    db.and_then(rusqlite::Error::sqlite_error_code)
        .is_some_and(crate::disk::refused)
}

/// Opens a session as the apps do: trash past its time goes, notes left
/// blank and the empty folders of imports cut short, then the files they had.
fn open(vault: Vault, mut store: Store) -> CommandResult<Value> {
    store.purge_expired_trash()?;
    store.discard_blank_notes()?;
    store.discard_left_imports()?;
    let blobs = vault.blobs(&mut store)?;
    match clean_files(&mut store, &blobs) {
        // a storage too full to clean up still opens, for files to be removed
        Err(err) if refused(&err) => {}
        cleaned => cleaned?,
    }
    SESSION.with_borrow_mut(|session| {
        *session = Some(Session {
            vault,
            store,
            blobs,
        });
    });
    Ok(Value::Null)
}

/// The web version has no device to keep a key in: a password or nothing.
const NO_KEYS: Keys<'static> = Keys {
    device: None,
    guarded: None,
};

fn status() -> CommandResult<Value> {
    let unlocked = SESSION.with_borrow(Option::is_some);
    let disk = disk();
    if !Vault::exists_on(disk.as_ref(), dir()) {
        return answer(VaultStatus::none(unlocked, &NO_KEYS));
    }
    let vault = Vault::open_on(disk, dir())?;
    answer(VaultStatus::of(&vault, unlocked, &NO_KEYS))
}

fn create(a: &Args) -> CommandResult<Value> {
    let way: Way = a.get("way")?;
    let password: String = a.get("password")?;
    let (vault, store) = vault::create(disk(), dir(), way, &password, None, &NO_KEYS)?;
    open(vault, store)
}

fn unlock(a: &Args) -> CommandResult<Value> {
    let password: Option<String> = a.get("password")?;
    let vault = Vault::open_on(disk(), dir())?;
    let store = vault::unlock(&vault, password.as_deref(), &NO_KEYS)?;
    open(vault, store)
}

/// Encrypts the open vault with a password. A failed try leaves the notes
/// open as they were.
fn encrypt(a: &Args) -> CommandResult<Value> {
    let password: Option<String> = a.get("password")?;
    let password = password
        .filter(|p| !p.is_empty())
        .ok_or_else(vault::needs_password)?;
    let Some(session) = SESSION.with_borrow_mut(Option::take) else {
        return Err(CommandError::locked());
    };
    drop(session.blobs);
    match vault::encrypt(session.vault, session.store, Some(&password), None) {
        Ok((vault, store)) => open(vault, store),
        Err(failed) => {
            if let Some((vault, store)) = failed.open {
                open(vault, store)?;
            }
            Err(failed.error.into())
        }
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
            answer(vault::check_password(&vault, &password))
        })(),
        "vault_create_recovery" => (|| {
            let password: Option<String> = a.get("password")?;
            let mut vault = Vault::open_on(disk(), dir())?;
            let code = vault::create_recovery(&mut vault, password.as_deref(), &NO_KEYS)?;
            refresh()?;
            answer(code)
        })(),
        "vault_remove_recovery" => (|| {
            let password: Option<String> = a.get("password")?;
            let mut vault = Vault::open_on(disk(), dir())?;
            vault::remove_recovery(&mut vault, password.as_deref(), &NO_KEYS)?;
            refresh()?;
            Ok(Value::Null)
        })(),
        "vault_recover" => (|| {
            let code: String = a.get("code")?;
            let new_password: String = a.get("newPassword")?;
            let mut vault = Vault::open_on(disk(), dir())?;
            let store = vault::recover(&mut vault, &code, &new_password, &NO_KEYS)?;
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
        "note_markdown" => {
            id().and_then(|id| with(|s| Ok(app_core::export::note_file(&s.store.note(id)?))))
        }
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
        "set_note_tags" => (|| {
            let (id, tags): (NoteId, Vec<String>) = (id()?, a.get("tags")?);
            with(|s| s.store.set_note_tags(id, &tags))
        })(),
        "trash_note" => id().and_then(|id| with(|s| s.store.trash_note(id))),
        "restore_note" => id().and_then(|id| with(|s| s.store.restore_note(id))),
        "purge_note" => id().and_then(|id| {
            with(|s| {
                s.store.purge_note(id)?;
                clean_files(&mut s.store, &s.blobs)
            })
        }),
        "discard_if_blank" => id().and_then(|id| with(|s| s.store.discard_if_blank(id))),
        "empty_trash" => with(|s| {
            let now = app_core::clock::Clock::now(&app_core::clock::SystemClock);
            let purged = s.store.purge_trashed_before(now)?;
            clean_files(&mut s.store, &s.blobs)?;
            Ok(purged)
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

fn tag_command(name: &str, a: &Args) -> Option<CommandResult<Value>> {
    let tag = || a.get::<TagId>("id");
    Some(match name {
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
        "remove_attachment" => a.get::<AttachmentId>("id").and_then(|id| {
            with(|s| {
                s.store.remove_attachment(id)?;
                clean_files(&mut s.store, &s.blobs)
            })
        }),
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
        .or_else(|| tag_command(name, a))
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
