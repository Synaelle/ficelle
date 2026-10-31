-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Attachments sync: their descriptions go into the space documents, the
-- files themselves to the server. The rust step after this file records the
-- attachments already in a vault.

ALTER TABLE attachments ADD COLUMN thumb_sha256 TEXT;
-- e2ee shared spaces: the epoch whose key seals the file
ALTER TABLE attachments ADD COLUMN epoch INTEGER;
-- removed attachments stay a while, so the trash, history or an undo can
-- still find their file, and so a removal merges with a rename elsewhere
ALTER TABLE attachments ADD COLUMN removed_at INTEGER;

DROP INDEX attachments_note_name;
CREATE UNIQUE INDEX attachments_note_name ON attachments (note_id, file_name)
    WHERE removed_at IS NULL;

CREATE TRIGGER sync_attachment_insert AFTER INSERT ON attachments
WHEN NOT EXISTS (SELECT 1 FROM sync_quiet)
BEGIN
    INSERT OR IGNORE INTO sync_touched (kind, space, id)
        SELECT 'note', space_id, id FROM notes WHERE id = NEW.note_id;
END;

CREATE TRIGGER sync_attachment_update AFTER UPDATE ON attachments
WHEN NOT EXISTS (SELECT 1 FROM sync_quiet)
BEGIN
    INSERT OR IGNORE INTO sync_touched (kind, space, id)
        SELECT 'note', space_id, id FROM notes WHERE id = NEW.note_id;
    INSERT OR IGNORE INTO sync_touched (kind, space, id)
        SELECT 'note', space_id, id FROM notes WHERE id = OLD.note_id;
END;

-- files the server holds for a space, by content hash. What a space's notes
-- use and is not here waits to be uploaded, what is here and no longer used
-- is released
CREATE TABLE files_on_server (
    space   TEXT NOT NULL,
    sha256  TEXT NOT NULL,
    PRIMARY KEY (space, sha256)
) WITHOUT ROWID;
