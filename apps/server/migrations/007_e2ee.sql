-- SPDX-License-Identifier: AGPL-3.0-or-later
-- End-to-end encrypted accounts: their OPAQUE record instead of a password
-- hash, and their keys, wrapped on the devices. The server opens neither.

ALTER TABLE users ADD COLUMN kind TEXT NOT NULL DEFAULT 'standard' CHECK (kind IN ('standard', 'e2ee'));
-- the OPAQUE registration record, e2ee accounts only
ALTER TABLE users ADD COLUMN opaque_record BLOB;
-- json key bundle: the master key wrapped by the password and by the recovery code
ALTER TABLE users ADD COLUMN key_bundle TEXT;
-- sha-256 of the recovery verifier, to check a recovery code without seeing it
ALTER TABLE users ADD COLUMN recovery_verifier BLOB;

-- standard, e2ee, or choice: what sign-up creates
INSERT INTO settings (key, value) VALUES ('account_kinds', 'choice');
