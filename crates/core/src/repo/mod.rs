// SPDX-License-Identifier: AGPL-3.0-or-later

//! The settings of this device's scheduled backups, and those the snapshot
//! backups of earlier versions left in the vault.

mod settings;

pub(crate) use settings::{BackupConfig, NO_PASSWORD, TargetConfig};
pub use settings::{BackupStatus, Schedule, folder_name};
