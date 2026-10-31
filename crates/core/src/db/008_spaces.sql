-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Several spaces in a vault: the personal one, and each space shared with
-- this account or by it. Every note and folder belongs to one space, '' is
-- the personal space. Documents of the personal space keep their bare id,
-- those of a shared space are stored as '<space id>/<document id>'.

ALTER TABLE notes ADD COLUMN space_id TEXT NOT NULL DEFAULT '';
ALTER TABLE folders ADD COLUMN space_id TEXT NOT NULL DEFAULT '';
CREATE INDEX notes_space ON notes (space_id);
CREATE INDEX folders_space ON folders (space_id);

-- shared spaces this account is in, with where its sync stands in each
CREATE TABLE spaces (
    id          TEXT PRIMARY KEY,
    -- the name the owner gave it, shown in the sidebar
    name        TEXT NOT NULL DEFAULT '',
    -- owner, edit or read
    role        TEXT NOT NULL,
    -- standard or e2ee
    kind        TEXT NOT NULL,
    owner_name  TEXT NOT NULL DEFAULT '',
    cursor      INTEGER NOT NULL DEFAULT 0,
    cursor_id   TEXT
) WITHOUT ROWID;

-- e2ee: a shared space's key for each epoch, opened from this account's grant
CREATE TABLE space_keys (
    space_id    TEXT NOT NULL REFERENCES spaces (id) ON DELETE CASCADE,
    epoch       INTEGER NOT NULL,
    key         BLOB NOT NULL,
    PRIMARY KEY (space_id, epoch)
) WITHOUT ROWID;

-- what local writes touched now names the space too
DROP TABLE sync_touched;
CREATE TABLE sync_touched (
    kind        TEXT NOT NULL,
    space       TEXT NOT NULL,
    id          TEXT NOT NULL,
    PRIMARY KEY (kind, space, id)
) WITHOUT ROWID;

DROP TRIGGER sync_note_insert;
DROP TRIGGER sync_note_update;
DROP TRIGGER sync_note_delete;
DROP TRIGGER sync_note_tag_insert;
DROP TRIGGER sync_note_tag_delete;
DROP TRIGGER sync_tag_rename;
DROP TRIGGER sync_folder_insert;
DROP TRIGGER sync_folder_update;
DROP TRIGGER sync_folder_delete;

CREATE TRIGGER sync_note_insert AFTER INSERT ON notes
WHEN NOT EXISTS (SELECT 1 FROM sync_quiet)
BEGIN
    INSERT OR IGNORE INTO sync_touched (kind, space, id) VALUES ('note', NEW.space_id, NEW.id);
END;

-- a note moved to another space is recorded in both: gone from one, new in
-- the other
CREATE TRIGGER sync_note_update AFTER UPDATE OF folder_id, title, body, pinned, created_at, updated_at, trashed_at, space_id ON notes
WHEN NOT EXISTS (SELECT 1 FROM sync_quiet)
BEGIN
    INSERT OR IGNORE INTO sync_touched (kind, space, id) VALUES ('note', NEW.space_id, NEW.id);
    INSERT OR IGNORE INTO sync_touched (kind, space, id) VALUES ('note', OLD.space_id, OLD.id);
END;

CREATE TRIGGER sync_note_delete AFTER DELETE ON notes
WHEN NOT EXISTS (SELECT 1 FROM sync_quiet)
BEGIN
    INSERT OR IGNORE INTO sync_touched (kind, space, id) VALUES ('note', OLD.space_id, OLD.id);
END;

CREATE TRIGGER sync_note_tag_insert AFTER INSERT ON note_tags
WHEN NOT EXISTS (SELECT 1 FROM sync_quiet)
BEGIN
    INSERT OR IGNORE INTO sync_touched (kind, space, id)
        SELECT 'note', space_id, id FROM notes WHERE id = NEW.note_id;
END;

-- a note deleted takes its tags with it: the note's own trigger records it
CREATE TRIGGER sync_note_tag_delete AFTER DELETE ON note_tags
WHEN NOT EXISTS (SELECT 1 FROM sync_quiet)
BEGIN
    INSERT OR IGNORE INTO sync_touched (kind, space, id)
        SELECT 'note', space_id, id FROM notes WHERE id = OLD.note_id;
END;

CREATE TRIGGER sync_tag_rename AFTER UPDATE OF name ON tags
WHEN NOT EXISTS (SELECT 1 FROM sync_quiet)
BEGIN
    INSERT OR IGNORE INTO sync_touched (kind, space, id)
        SELECT 'note', n.space_id, n.id FROM note_tags t JOIN notes n ON n.id = t.note_id
        WHERE t.tag_id = NEW.id;
END;

CREATE TRIGGER sync_folder_insert AFTER INSERT ON folders
WHEN NOT EXISTS (SELECT 1 FROM sync_quiet)
BEGIN
    INSERT OR IGNORE INTO sync_touched (kind, space, id) VALUES ('folders', NEW.space_id, '*');
END;

CREATE TRIGGER sync_folder_update AFTER UPDATE ON folders
WHEN NOT EXISTS (SELECT 1 FROM sync_quiet)
BEGIN
    INSERT OR IGNORE INTO sync_touched (kind, space, id) VALUES ('folders', NEW.space_id, '*');
    INSERT OR IGNORE INTO sync_touched (kind, space, id) VALUES ('folders', OLD.space_id, '*');
END;

CREATE TRIGGER sync_folder_delete AFTER DELETE ON folders
WHEN NOT EXISTS (SELECT 1 FROM sync_quiet)
BEGIN
    INSERT OR IGNORE INTO sync_touched (kind, space, id) VALUES ('folders', OLD.space_id, '*');
END;
