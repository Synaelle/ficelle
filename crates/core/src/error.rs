// SPDX-License-Identifier: AGPL-3.0-or-later

//! Error type shared by the whole crate.

/// Everything that can go wrong in the core.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The database refused a query.
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    /// Reading or writing a file failed.
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),

    /// The password does not unlock this vault.
    #[error("wrong password")]
    WrongPassword,

    /// The database key does not open the database, or the file is damaged.
    #[error("the database could not be opened with this key")]
    WrongKey,

    /// This device has no remembered key for the vault, a password is needed.
    #[error("this device does not remember the key, enter the password")]
    NotRemembered,

    /// A row that should exist does not.
    #[error("{0} not found")]
    NotFound(&'static str),

    /// A request that breaks a rule, for example moving a folder into itself.
    #[error("{0}")]
    Invalid(String),

    /// The operating system key store failed.
    #[error("secure storage error: {0}")]
    KeyStore(String),

    /// Data on disk does not have the expected shape.
    #[error("damaged data: {0}")]
    Corrupt(String),

    /// Something would go over a size limit, in bytes.
    #[error("over the limit of {0} bytes")]
    TooLarge(usize),

    /// The user dismissed a biometric or system verification prompt.
    #[error("verification was cancelled")]
    Cancelled,

    /// The system random generator or a cipher failed.
    #[error("cryptography error")]
    Crypto,

    /// A backup target refused or could not be reached.
    #[error("{0}")]
    Target(String),
}

impl From<app_repo::Error> for Error {
    fn from(e: app_repo::Error) -> Self {
        use app_repo::Error as R;
        match e {
            R::Io(e) => Self::Io(e),
            R::WrongPassword => Self::WrongPassword,
            R::NotFound(what) => Self::NotFound(what),
            R::Invalid(why) => Self::Invalid(why),
            R::Corrupt(why) => Self::Corrupt(why),
            R::Target(why) => Self::Target(why),
            R::Crypto => Self::Crypto,
        }
    }
}

/// Result alias used across the crate.
pub type Result<T> = std::result::Result<T, Error>;
