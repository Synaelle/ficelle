-- SPDX-License-Identifier: AGPL-3.0-or-later
-- An outbox entry can be a whole document that replaces, on the server, all
-- of its changes up to this number (this device had taken them all).

ALTER TABLE sync_outbox ADD COLUMN compacts INTEGER;
