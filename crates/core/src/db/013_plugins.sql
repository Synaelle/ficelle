-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Installed plugins and what each keeps for itself. In the vault, so
-- encrypted with it, and on this device only: plugins are not synced.

CREATE TABLE plugins (
    id          TEXT PRIMARY KEY,
    manifest    TEXT NOT NULL,
    code        TEXT NOT NULL,
    locales     TEXT NOT NULL,
    sha256      TEXT NOT NULL,
    -- the permissions the person approved, a json list
    granted     TEXT NOT NULL,
    enabled     INTEGER NOT NULL,
    installed_at INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE plugin_data (
    plugin  TEXT NOT NULL REFERENCES plugins(id) ON DELETE CASCADE,
    key     TEXT NOT NULL,
    value   TEXT NOT NULL,
    PRIMARY KEY (plugin, key)
) WITHOUT ROWID;
