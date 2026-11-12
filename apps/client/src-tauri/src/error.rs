// SPDX-License-Identifier: AGPL-3.0-or-later

//! Errors sent to the ui. `kind` is stable so the ui can show a translated
//! message, `message` is the English detail for logs.

use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    kind: &'static str,
    message: String,
}

impl CommandError {
    /// What went wrong, in words.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// What kind of thing went wrong.
    pub fn kind(&self) -> &'static str {
        self.kind
    }

    pub fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn locked() -> Self {
        Self::new("locked", "the vault is locked")
    }
}

impl From<app_core::Error> for CommandError {
    fn from(err: app_core::Error) -> Self {
        Self::new(err.kind(), err.to_string())
    }
}

pub type CommandResult<T> = Result<T, CommandError>;
