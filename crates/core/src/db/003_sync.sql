-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Sync. Notes, folders and tags also live in CRDT documents (Loro): one per
-- note for its title and body, one "space" for the folder tree and where
-- each note sits. The rows above stay the fast local view for lists and
-- search, rebuilt from the documents when another device changes them. The
-- rust step after this file builds the documents of an existing vault.

-- a document as of its last compaction
CREATE TABLE sync_docs (
    id          TEXT PRIMARY KEY,
    snapshot    BLOB NOT NULL
) WITHOUT ROWID;

-- changes since the snapshot, folded into it now and then
CREATE TABLE sync_updates (
    seq         INTEGER PRIMARY KEY,
    doc_id      TEXT NOT NULL,
    bytes       BLOB NOT NULL
);
CREATE INDEX sync_updates_doc ON sync_updates (doc_id, seq);

-- changes made here and not yet taken by the server, in order. seq never
-- repeats, even once the outbox was emptied: with the device's peer id it
-- names the change for the server, which drops a name it already has
CREATE TABLE sync_outbox (
    seq         INTEGER PRIMARY KEY AUTOINCREMENT,
    doc_id      TEXT NOT NULL,
    bytes       BLOB NOT NULL,
    created_at  INTEGER NOT NULL
);

-- what local writes touched, recorded into the documents before commit
CREATE TABLE sync_touched (
    kind        TEXT NOT NULL,
    id          TEXT NOT NULL,
    PRIMARY KEY (kind, id)
) WITHOUT ROWID;

-- set while rows are rebuilt from the documents, so that is not recorded back
CREATE TABLE sync_quiet (
    on_         INTEGER NOT NULL
);

CREATE TRIGGER sync_note_insert AFTER INSERT ON notes
WHEN NOT EXISTS (SELECT 1 FROM sync_quiet)
BEGIN
    INSERT OR IGNORE INTO sync_touched (kind, id) VALUES ('note', NEW.id);
END;

CREATE TRIGGER sync_note_update AFTER UPDATE OF folder_id, title, body, pinned, created_at, updated_at, trashed_at ON notes
WHEN NOT EXISTS (SELECT 1 FROM sync_quiet)
BEGIN
    INSERT OR IGNORE INTO sync_touched (kind, id) VALUES ('note', NEW.id);
END;

CREATE TRIGGER sync_note_delete AFTER DELETE ON notes
WHEN NOT EXISTS (SELECT 1 FROM sync_quiet)
BEGIN
    INSERT OR IGNORE INTO sync_touched (kind, id) VALUES ('note', OLD.id);
END;

CREATE TRIGGER sync_note_tag_insert AFTER INSERT ON note_tags
WHEN NOT EXISTS (SELECT 1 FROM sync_quiet)
BEGIN
    INSERT OR IGNORE INTO sync_touched (kind, id) VALUES ('note', NEW.note_id);
END;

CREATE TRIGGER sync_note_tag_delete AFTER DELETE ON note_tags
WHEN NOT EXISTS (SELECT 1 FROM sync_quiet)
BEGIN
    INSERT OR IGNORE INTO sync_touched (kind, id) VALUES ('note', OLD.note_id);
END;

-- a tag renamed changes every note carrying it
CREATE TRIGGER sync_tag_rename AFTER UPDATE OF name ON tags
WHEN NOT EXISTS (SELECT 1 FROM sync_quiet)
BEGIN
    INSERT OR IGNORE INTO sync_touched (kind, id)
        SELECT 'note', note_id FROM note_tags WHERE tag_id = NEW.id;
END;

CREATE TRIGGER sync_folder_insert AFTER INSERT ON folders
WHEN NOT EXISTS (SELECT 1 FROM sync_quiet)
BEGIN
    INSERT OR IGNORE INTO sync_touched (kind, id) VALUES ('folders', '*');
END;

CREATE TRIGGER sync_folder_update AFTER UPDATE ON folders
WHEN NOT EXISTS (SELECT 1 FROM sync_quiet)
BEGIN
    INSERT OR IGNORE INTO sync_touched (kind, id) VALUES ('folders', '*');
END;

CREATE TRIGGER sync_folder_delete AFTER DELETE ON folders
WHEN NOT EXISTS (SELECT 1 FROM sync_quiet)
BEGIN
    INSERT OR IGNORE INTO sync_touched (kind, id) VALUES ('folders', '*');
END;
