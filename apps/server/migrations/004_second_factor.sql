-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Authenticator app codes, recovery codes, and the short lived challenges
-- that carry a sign-in from one request to the next.

-- base32 secret, active once confirmed with a first code
ALTER TABLE users ADD COLUMN totp_secret TEXT;
-- secret waiting for its first code
ALTER TABLE users ADD COLUMN totp_pending TEXT;
-- last 30 second step accepted, a code never works twice
ALTER TABLE users ADD COLUMN totp_last_step INTEGER;

-- sha-256 of each code, they carry 60 random bits
CREATE TABLE recovery_codes (
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    hash BLOB NOT NULL,
    used_at INTEGER,
    PRIMARY KEY (user_id, hash)
) STRICT;

-- one step of a sign-in or ceremony in progress, found by the sha-256 of
-- its token. `data` is json whose shape depends on `kind`.
CREATE TABLE challenges (
    hash BLOB PRIMARY KEY,
    kind TEXT NOT NULL,
    user_id TEXT REFERENCES users (id) ON DELETE CASCADE,
    data TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    expires_at INTEGER NOT NULL
) STRICT;
CREATE INDEX challenges_expiry ON challenges (expires_at);
