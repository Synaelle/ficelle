// SPDX-License-Identifier: AGPL-3.0-or-later

//! Typed identifiers.
//!
//! Ids are UUID v7: generated on any device without coordination, unique
//! across devices once sync arrives, and roughly ordered by creation time so
//! database indexes stay compact. Each kind of row gets its own type so a note
//! id can never be passed where a folder id is expected.

use std::fmt;
use std::str::FromStr;

use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// A new, unique id.
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(s).map(Self)
            }
        }

        impl ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
                Ok(ToSqlOutput::from(self.0.to_string()))
            }
        }

        impl FromSql for $name {
            fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
                let text = value.as_str()?;
                Uuid::parse_str(text)
                    .map(Self)
                    .map_err(|err| FromSqlError::Other(Box::new(err)))
            }
        }
    };
}

id_type!(
    /// Identifies a note.
    NoteId
);
id_type!(
    /// Identifies a folder.
    FolderId
);
id_type!(
    /// Identifies a tag.
    TagId
);
id_type!(
    /// Identifies a saved version of a note.
    VersionId
);
id_type!(
    /// Identifies an attachment.
    AttachmentId
);
id_type!(
    /// Identifies a vault, the folder that holds one local database.
    VaultId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_round_trip_as_text() {
        let a = NoteId::new();
        let b = NoteId::new();
        assert_ne!(a, b);
        assert_eq!(a.to_string().parse::<NoteId>().unwrap(), a);
    }

    #[test]
    fn later_ids_sort_after_earlier_ones() {
        let first = NoteId::new();
        let second = NoteId::new();
        assert!(first < second);
    }
}
