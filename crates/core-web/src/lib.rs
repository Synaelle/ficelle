// SPDX-License-Identifier: AGPL-3.0-or-later

//! The notes core as WebAssembly, for the web version. It runs in a worker:
//! the browser's private file storage it keeps the notes in is only reached
//! from one, and long work (a password's key, an import) never freezes the
//! page. The page sends the same commands the apps do, by name with their
//! arguments as json, and gets json back, or `{kind, message}` as the error.

#![cfg(target_family = "wasm")]

mod commands;
mod disk;
mod files;

use std::cell::RefCell;

use app_core::blobs::BlobStore;
use app_core::store::Store;
use app_core::vault::Vault;
use serde::Serialize;
use serde_json::Value;
use sqlite_wasm_rs::WasmOsCallback;
use sqlite_wasm_vfs::sahpool::{OpfsSAHPoolCfgBuilder, install};
use wasm_bindgen::prelude::*;

/// An unlocked vault.
struct Session {
    vault: Vault,
    store: Store,
    blobs: BlobStore,
}

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
}

/// An error as the page reads it, the apps' shape.
#[derive(Debug, Serialize)]
struct CommandError {
    kind: &'static str,
    message: String,
}

impl CommandError {
    fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    fn locked() -> Self {
        Self::new("locked", "the vault is locked")
    }

    fn unsupported() -> Self {
        Self::new("unsupported", "not in the web version")
    }
}

impl From<app_core::Error> for CommandError {
    fn from(err: app_core::Error) -> Self {
        Self::new(err.kind(), err.to_string())
    }
}

impl From<CommandError> for JsError {
    fn from(err: CommandError) -> Self {
        Self::new(&serde_json::to_string(&err).unwrap_or(err.message))
    }
}

type CommandResult<T> = Result<T, CommandError>;

/// Makes the browser's private file storage SQLite's, for the notes and
/// their files, in the folder of `scope`: the path the page is served from,
/// encoded. Two web versions on one address (a public one at /web, a
/// server's at /app) keep their notes apart, each in its own folder.
///
/// # Errors
///
/// Not a scope (none, or not one the page makes), outside a worker, or while
/// another tab holds the storage.
#[wasm_bindgen]
pub async fn start(scope: &str) -> Result<(), JsError> {
    let directory = app_core::disk::web_scope_folder(scope)
        .ok_or_else(|| CommandError::new("storage", "the page gave no scope for its notes"))?;
    let config = OpfsSAHPoolCfgBuilder::new().directory(&directory).build();
    let pool = install::<WasmOsCallback>(&config, true)
        .await
        .map_err(|e| CommandError::new("storage", e.to_string()))?;
    disk::start(Box::new(pool)).map_err(CommandError::from)?;
    Ok(())
}

/// As [`start`], with the notes in memory, gone when the page goes: for
/// the preview of development and the tests, which have no private file
/// storage or keep nothing.
///
/// # Errors
///
/// SQLite could not start.
#[wasm_bindgen(js_name = startInMemory)]
pub fn start_in_memory() -> Result<(), JsError> {
    let files = app_sqlite_memory::memory_files()
        .map_err(|e| CommandError::new("storage", e.to_string()))?;
    disk::start(Box::new(files)).map_err(CommandError::from)?;
    Ok(())
}

/// Runs the command `name` with its arguments, json in and out.
///
/// # Errors
///
/// The command's error, `{kind, message}` as json.
#[wasm_bindgen]
pub fn call(name: &str, args: &str) -> Result<String, JsError> {
    let args: Value =
        serde_json::from_str(args).map_err(|e| CommandError::new("invalid", e.to_string()))?;
    let answer = commands::run(name, &commands::Args(args), None)?;
    Ok(answer.to_string())
}

/// As [`call`], for the commands that take bytes (a file to attach, a
/// backup to restore).
///
/// # Errors
///
/// As [`call`].
#[wasm_bindgen(js_name = callBytes)]
pub fn call_bytes(name: &str, args: &str, bytes: Vec<u8>) -> Result<String, JsError> {
    let args: Value =
        serde_json::from_str(args).map_err(|e| CommandError::new("invalid", e.to_string()))?;
    let answer = commands::run(name, &commands::Args(args), Some(bytes))?;
    Ok(answer.to_string())
}

/// Runs a command that answers with bytes (a file, a backup, an export).
///
/// # Errors
///
/// As [`call`].
#[wasm_bindgen(js_name = readBytes)]
pub fn read_bytes(name: &str, args: &str) -> Result<Vec<u8>, JsError> {
    let args: Value =
        serde_json::from_str(args).map_err(|e| CommandError::new("invalid", e.to_string()))?;
    Ok(commands::bytes(name, &commands::Args(args))?)
}
