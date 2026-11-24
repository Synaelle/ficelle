-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Sharing notes left the app. The step's code makes every shared space's
-- notes and folders the person's own, then drops the spaces' tables. The
-- account's key pair served sharing only.

ALTER TABLE sync_link DROP COLUMN key_public;
ALTER TABLE sync_link DROP COLUMN key_private;
