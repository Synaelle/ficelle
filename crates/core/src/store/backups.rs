// SPDX-License-Identifier: AGPL-3.0-or-later

//! This device's backup settings and their status, in the vault (see
//! [`crate::repo`]).

use rusqlite::{OptionalExtension, params};

use super::Store;
use crate::error::{Error, Result};
use crate::repo::{BackupConfig, BackupStatus};

impl Store {
    fn backup_row(&self) -> Result<(Option<String>, String)> {
        Ok(self
            .conn
            .query_row(
                "SELECT config, status FROM backup_settings WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .unwrap_or((None, "{}".into())))
    }

    /// Where and how this device backs up, None when it does not.
    ///
    /// # Errors
    ///
    /// Database errors, damaged settings.
    pub fn backup_config(&self) -> Result<Option<BackupConfig>> {
        self.backup_row()?
            .0
            .map(|text| serde_json::from_str(&text).map_err(|e| Error::Corrupt(e.to_string())))
            .transpose()
    }

    /// Sets or clears the backup settings. Clearing keeps the status.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn set_backup_config(&self, config: Option<&BackupConfig>) -> Result<()> {
        let text = config
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| Error::Invalid(e.to_string()))?;
        self.conn.execute(
            "INSERT INTO backup_settings (id, config) VALUES (1, ?1)
             ON CONFLICT (id) DO UPDATE SET config = excluded.config",
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
        // a status from an older version reads as far as it goes
        Ok(serde_json::from_str(&self.backup_row()?.1).unwrap_or_default())
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
    use crate::repo::{BackupConfig, BackupStatus, Policy, Schedule, TargetConfig};

    #[test]
    fn settings_and_status_are_kept_apart() {
        let (s, _) = store();
        assert_eq!(s.backup_config().unwrap(), None);
        assert_eq!(s.backup_status().unwrap(), BackupStatus::default());
        let config = BackupConfig {
            target: TargetConfig::Folder {
                path: "D:/Backups".into(),
            },
            password: "pw".into(),
            schedule: Schedule::Daily,
            policy: Policy::default(),
            host: "laptop".into(),
        };
        s.set_backup_config(Some(&config)).unwrap();
        let status = BackupStatus {
            last_run_at: Some(5),
            last_error: Some("unreachable".into()),
            ..BackupStatus::default()
        };
        s.set_backup_status(&status).unwrap();
        assert_eq!(s.backup_config().unwrap(), Some(config));
        s.set_backup_config(None).unwrap();
        assert_eq!(s.backup_config().unwrap(), None);
        assert_eq!(s.backup_status().unwrap(), status);
    }
}
