-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The web version's sign-in, a cookie for the browser: each one belongs to
-- the device made by signing in there, and ends when that device is signed
-- out. Only its hash is kept.

CREATE TABLE web_sessions (
    hash BLOB PRIMARY KEY,
    device_id TEXT NOT NULL REFERENCES devices (id) ON DELETE CASCADE,
    expires_at INTEGER NOT NULL
) STRICT;

CREATE INDEX web_sessions_device ON web_sessions (device_id);
