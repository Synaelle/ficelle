-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Apps signing in with a code typed on the server's page, on the same
-- device or another one. A code lives ten minutes, only its hash is kept.

CREATE TABLE app_links (
    hash BLOB PRIMARY KEY,
    challenge TEXT NOT NULL,
    app_key TEXT NOT NULL,
    device_name TEXT NOT NULL,
    device_platform TEXT NOT NULL,
    -- set when the person approves it
    user_id TEXT REFERENCES users (id) ON DELETE CASCADE,
    -- e2ee: the export key sealed to the app, passed along untouched
    sealed TEXT,
    expires_at INTEGER NOT NULL
) STRICT;
