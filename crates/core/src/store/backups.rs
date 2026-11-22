// SPDX-License-Identifier: AGPL-3.0-or-later

//! This device's backup settings and their status, in the vault (see
//! [`crate::auto_backup`]).

use rusqlite::{OptionalExtension, params};

use super::Store;
use crate::auto_backup::AutoBackups;
use crate::error::{Error, Result};
use crate::repo::BackupStatus;

impl Store {
    /// This device's automatic backups, None when it makes none.
    ///
    /// # Errors
    ///
    /// Database errors, damaged settings.
    pub fn auto_backups(&self) -> Result<Option<AutoBackups>> {
        let text: Option<String> = self
            .conn
            .query_row("SELECT files FROM backup_settings WHERE id = 1", [], |r| {
                r.get(0)
            })
            .optional()?
            .flatten();
        text.map(|text| serde_json::from_str(&text).map_err(|e| Error::Corrupt(e.to_string())))
            .transpose()
    }

    /// Sets or clears the automatic backups. Clearing keeps the status.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn set_auto_backups(&self, config: Option<&AutoBackups>) -> Result<()> {
        let text = config
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| Error::Invalid(e.to_string()))?;
        self.conn.execute(
            "INSERT INTO backup_settings (id, files) VALUES (1, ?1)
             ON CONFLICT (id) DO UPDATE SET files = excluded.files",
            params![text],
        )?;
        Ok(())
    }

    /// How the runs went.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn backup_status(&self) -> Result<BackupStatus> {
        let text: String = self
            .conn
            .query_row("SELECT status FROM backup_settings WHERE id = 1", [], |r| {
                r.get(0)
            })
            .optional()?
            .unwrap_or_else(|| "{}".into());
        // a status from an older version reads as far as it goes
        Ok(serde_json::from_str(&text).unwrap_or_default())
    }

    /// Records how the runs went.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn set_backup_status(&self, status: &BackupStatus) -> Result<()> {
        let text = serde_json::to_string(status).map_err(|e| Error::Invalid(e.to_string()))?;
        self.conn.execute(
            "INSERT INTO backup_settings (id, status) VALUES (1, ?1)
             ON CONFLICT (id) DO UPDATE SET status = excluded.status",
            params![text],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::store;
    use crate::auto_backup::{AutoBackups, DEFAULT_KEEP};
    use crate::repo::{BackupStatus, Schedule};

    #[test]
    fn settings_and_status_are_kept_apart() {
        let (s, _) = store();
        assert_eq!(s.auto_backups().unwrap(), None);
        assert_eq!(s.backup_status().unwrap(), BackupStatus::default());
        let config = AutoBackups {
            folder: "D:/Backups".into(),
            schedule: Schedule::Daily,
            keep: DEFAULT_KEEP,
            password: Some("pw".into()),
        };
        s.set_auto_backups(Some(&config)).unwrap();
        let status = BackupStatus {
            last_run_at: Some(5),
            last_error: Some("unreachable".into()),
            ..BackupStatus::default()
        };
        s.set_backup_status(&status).unwrap();
        assert_eq!(s.auto_backups().unwrap(), Some(config));
        s.set_auto_backups(None).unwrap();
        assert_eq!(s.auto_backups().unwrap(), None);
        assert_eq!(s.backup_status().unwrap(), status);
    }
}
