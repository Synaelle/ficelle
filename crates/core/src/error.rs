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

    /// The vault's database file is not there, or holds nothing.
    #[error("the notes database is missing or empty")]
    NotesMissing,

    /// The vault's database file is there but damaged: the key that opens
    /// it is known to be right, or it needs none.
    #[error("the notes database is damaged")]
    NotesDamaged,

    /// Data on disk does not have the expected shape.
    #[error("damaged data: {0}")]
    Corrupt(String),

    /// The file is not a backup of this app at all.
    #[error("not a backup")]
    NotBackup,

    /// Something would go over a size limit, in bytes.
    #[error("over the limit of {0} bytes")]
    TooLarge(usize),

    /// The user dismissed a biometric or system verification prompt.
    #[error("verification was cancelled")]
    Cancelled,

    /// The system random generator or a cipher failed.
    #[error("cryptography error")]
    Crypto,
}

impl Error {
    /// The kind of error, as the pages name it to say what went wrong.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Db(err) if damaged(err) => "notesDamaged",
            Self::NotesDamaged => "notesDamaged",
            Self::Db(_) => "database",
            Self::Io(_) => "file",
            Self::WrongPassword => "wrongPassword",
            Self::WrongKey => "wrongKey",
            Self::NotRemembered => "notRemembered",
            Self::NotFound(_) => "notFound",
            Self::Invalid(_) => "invalid",
            Self::KeyStore(_) => "keyStore",
            Self::NotesMissing => "notesMissing",
            Self::Corrupt(_) => "corrupt",
            Self::NotBackup => "notBackup",
            Self::Cancelled => "cancelled",
            Self::TooLarge(_) => "tooLarge",
            Self::Crypto => "crypto",
        }
    }
}

/// Whether SQLite found the file damaged, at opening or on a later read.
fn damaged(err: &rusqlite::Error) -> bool {
    matches!(
        err.sqlite_error_code(),
        Some(rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase)
    )
}

/// Result alias used across the crate.
pub type Result<T> = std::result::Result<T, Error>;
