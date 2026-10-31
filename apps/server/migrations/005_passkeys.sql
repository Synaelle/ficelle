-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Passkeys and security keys. A passkey alone signs in, it also serves as
-- the second factor after a password.

CREATE TABLE passkeys (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    -- the authenticator's id for it, unique across accounts
    credential_id BLOB NOT NULL UNIQUE,
    -- public key and signature counter, as webauthn-rs stores them
    credential TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    last_used_at INTEGER
) STRICT;
CREATE INDEX passkeys_user ON passkeys (user_id);
