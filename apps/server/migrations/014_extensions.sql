-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Plugins and themes the admin offers to the instance's apps. The rules
-- for all of them are one json value in settings, key 'extensions'.

CREATE TABLE extensions (
    kind        TEXT NOT NULL CHECK (kind IN ('plugin', 'theme')),
    id          TEXT NOT NULL,
    name        TEXT NOT NULL,
    -- a plugin's manifest as json, {} for a theme
    meta        TEXT NOT NULL,
    content     BLOB NOT NULL,
    sha256      TEXT NOT NULL,
    preinstall  INTEGER NOT NULL,
    uploaded_at INTEGER NOT NULL,
    PRIMARY KEY (kind, id)
) STRICT;
