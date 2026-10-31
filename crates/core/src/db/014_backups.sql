-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Scheduled backups on this device: where to, with which password, when,
-- and how the last runs went. In the vault, so encrypted with it, and on
-- this device only.

CREATE TABLE backup_settings (
    id      INTEGER PRIMARY KEY CHECK (id = 1),
    config  TEXT,
    status  TEXT NOT NULL DEFAULT '{}'
);
