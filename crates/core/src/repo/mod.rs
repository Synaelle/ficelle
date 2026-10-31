// SPDX-License-Identifier: AGPL-3.0-or-later

//! Backup repositories (see `app_repo`), and what an app puts in them: its
//! notes, restored through the backup archive's path. The settings of this
//! device's scheduled backups.

pub use app_repo::{
    CheckReport, FolderTarget, Index, Item, KdfParams, Listed, Location, Lock, Policy, PruneReport,
    Repo, S3Config, Snapshot, Target, WebdavConfig, WriteStats, Writer, keep,
};

mod settings;
mod vault;

pub use settings::{BackupConfig, BackupStatus, Schedule, TargetConfig};
pub use vault::{
    RunReport, SnapshotContents, TestRestore, backup_data, backup_vault, fetch_files,
    read_snapshot, restore_vault, test_restore,
};

#[cfg(test)]
mod tests;
