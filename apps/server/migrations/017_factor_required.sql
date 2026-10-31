-- SPDX-License-Identifier: AGPL-3.0-or-later
-- An admin made from the command line or reset there has a password alone:
-- until it adds a passkey or an authenticator app, it signs in but acts as
-- an admin nowhere.
ALTER TABLE users ADD COLUMN factor_required INTEGER NOT NULL DEFAULT 0;
