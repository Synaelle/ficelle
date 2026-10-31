// SPDX-License-Identifier: AGPL-3.0-or-later

//! Commands the ui calls through `invoke`. Each one is a thin wrapper around
//! the core, all logic lives in `app-core`.

pub mod accent;
pub mod backup;
pub mod backups;
pub mod clock;
pub mod dev_report;
pub mod files;
pub mod fonts;
pub mod import;
pub mod links;
pub mod notes;
pub mod quick;
pub mod themes;
pub mod updates;
pub mod vault;
