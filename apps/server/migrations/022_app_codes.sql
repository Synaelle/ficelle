-- SPDX-License-Identifier: AGPL-3.0-or-later
-- One-use codes that hand a sign-in made on the server's page to the app
-- that opened it, for two minutes. Only their hash is kept.

CREATE TABLE app_codes (
    hash BLOB PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    device_name TEXT NOT NULL,
    device_platform TEXT NOT NULL,
    challenge TEXT NOT NULL,
    redirect TEXT NOT NULL,
    -- e2ee: the export key sealed to the app, passed along untouched
    sealed TEXT,
    expires_at INTEGER NOT NULL
) STRICT;
