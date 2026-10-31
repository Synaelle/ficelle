-- SPDX-License-Identifier: AGPL-3.0-or-later
-- E2ee sharing: this account's key pair, kept with the account master key.

ALTER TABLE sync_link ADD COLUMN key_public TEXT;
ALTER TABLE sync_link ADD COLUMN key_private BLOB;
