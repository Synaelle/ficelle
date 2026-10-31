-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Accounts, signed in devices and their tokens, invites, runtime settings
-- and the security log. Times are unix milliseconds. Tokens are stored as
-- sha-256 hashes only, a copy of the database signs nobody in.

CREATE TABLE users (
    id TEXT PRIMARY KEY,
    -- lower case, unique whatever the letter case
    username TEXT NOT NULL UNIQUE COLLATE NOCASE,
    display_name TEXT NOT NULL,
    -- PHC string, $argon2id$...
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('admin', 'user')),
    created_at INTEGER NOT NULL,
    disabled_at INTEGER,
    -- failed sign-ins since the last good one
    failed_logins INTEGER NOT NULL DEFAULT 0,
    -- locks so far, each one waits twice as long as the previous
    lock_count INTEGER NOT NULL DEFAULT 0,
    locked_until INTEGER
) STRICT;

CREATE TABLE devices (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    platform TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    revoked_at INTEGER
) STRICT;
CREATE INDEX devices_user ON devices (user_id);

CREATE TABLE access_tokens (
    hash BLOB PRIMARY KEY,
    device_id TEXT NOT NULL REFERENCES devices (id) ON DELETE CASCADE,
    expires_at INTEGER NOT NULL
) STRICT;
CREATE INDEX access_tokens_device ON access_tokens (device_id);

-- a used token stays until it expires: seeing it again means it was stolen
CREATE TABLE refresh_tokens (
    hash BLOB PRIMARY KEY,
    device_id TEXT NOT NULL REFERENCES devices (id) ON DELETE CASCADE,
    expires_at INTEGER NOT NULL,
    used_at INTEGER
) STRICT;
CREATE INDEX refresh_tokens_device ON refresh_tokens (device_id);

CREATE TABLE invites (
    id TEXT PRIMARY KEY,
    hash BLOB NOT NULL UNIQUE,
    created_by TEXT REFERENCES users (id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    used_at INTEGER,
    used_by TEXT REFERENCES users (id) ON DELETE SET NULL,
    revoked_at INTEGER
) STRICT;

-- runtime policy the admin changes without a restart
CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;
INSERT INTO settings (key, value) VALUES ('registration', 'closed');

CREATE TABLE audit_log (
    id INTEGER PRIMARY KEY,
    at INTEGER NOT NULL,
    event TEXT NOT NULL,
    user_id TEXT,
    device_id TEXT,
    ip TEXT,
    detail TEXT
) STRICT;
CREATE INDEX audit_log_at ON audit_log (at);
