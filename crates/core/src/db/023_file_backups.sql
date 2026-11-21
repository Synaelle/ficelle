-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Automatic backups become backup files in a folder (auto_backup.rs). The
-- snapshot backups' settings stay in `config`, only read now, to restore
-- from them; the folder they used is where the files go
-- (auto_backup::from_snapshot_folder, run after this).

ALTER TABLE backup_settings ADD COLUMN files TEXT;
