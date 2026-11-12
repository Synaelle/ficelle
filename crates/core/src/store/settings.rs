// SPDX-License-Identifier: AGPL-3.0-or-later

//! The person's choices that follow them to every device. The app names each
//! one and gives its value as JSON; the store keeps and syncs them without
//! reading them. A choice nobody made has no row, so it never travels.

use rusqlite::params;

use super::Store;
use crate::error::{Error, Result};

impl Store {
    /// Every choice made on some device, by name, each a JSON value.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn synced_settings(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT key, value FROM synced_settings ORDER BY key")?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Makes a choice on this device, for every device. The value is JSON.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] for an empty name or a value that is not JSON,
    /// database errors.
    pub fn set_synced_setting(&mut self, key: &str, value: &str) -> Result<()> {
        if key.is_empty() {
            return Err(Error::Invalid("a setting needs a name".into()));
        }
        if serde_json::from_str::<serde_json::Value>(value).is_err() {
            return Err(Error::Invalid("a setting's value must be JSON".into()));
        }
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO synced_settings (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value
             WHERE value IS NOT excluded.value",
            params![key, value],
        )?;
        crate::sync::finish(tx, &mut self.docs, self.clock.now())?;
        Ok(())
    }
}
