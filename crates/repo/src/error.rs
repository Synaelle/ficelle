// SPDX-License-Identifier: AGPL-3.0-or-later

//! What can go wrong with a repository.

/// Errors of a repository or its target.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Reading or writing a file failed.
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),

    /// The password does not open this repository.
    #[error("wrong password")]
    WrongPassword,

    /// Something that should be there is not.
    #[error("{0} not found")]
    NotFound(&'static str),

    /// A request that breaks a rule.
    #[error("{0}")]
    Invalid(String),

    /// The repository holds something it should not: damage, or another
    /// repository's files.
    #[error("damaged data: {0}")]
    Corrupt(String),

    /// The target answered with a refusal or could not be reached.
    #[error("{0}")]
    Target(String),

    /// The system random generator or a cipher failed.
    #[error("cryptography error")]
    Crypto,
}

/// Result alias of the crate.
pub type Result<T> = std::result::Result<T, Error>;
