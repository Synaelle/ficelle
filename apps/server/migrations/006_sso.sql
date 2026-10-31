-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Identities at the single sign-on provider, linked to accounts. An
-- account made through single sign-on has an empty password_hash.

CREATE TABLE sso_identities (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    -- what to show for it, the provider's email or username
    name TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE (issuer, subject)
) STRICT;
CREATE INDEX sso_identities_user ON sso_identities (user_id);
