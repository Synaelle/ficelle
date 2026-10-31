-- SPDX-License-Identifier: AGPL-3.0-or-later
-- An optional email per account, kept only once confirmed, for password
-- resets and notices. And the date an account asked to be deleted at.

ALTER TABLE users ADD COLUMN email TEXT;
ALTER TABLE users ADD COLUMN email_verified_at INTEGER;
-- set while a deletion is pending, the account goes at this time
ALTER TABLE users ADD COLUMN delete_at INTEGER;
CREATE INDEX users_delete_at ON users (delete_at) WHERE delete_at IS NOT NULL;
