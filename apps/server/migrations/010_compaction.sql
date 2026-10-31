-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Compaction: a device sends one change holding all of a document up to a
-- number, the changes it replaces are emptied. Their rows stay, empty, so a
-- resend of one is still known and a device's cursor still names a change.

ALTER TABLE sync_changes ADD COLUMN compacted INTEGER NOT NULL DEFAULT 0;
CREATE INDEX sync_changes_doc ON sync_changes (space_id, doc_id, seq);
