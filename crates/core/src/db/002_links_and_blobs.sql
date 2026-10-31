-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Wiki link index and per vault secrets. Existing notes are indexed by the
-- rust step that runs right after this file (see db.rs).

-- lowercased trimmed title, kept by rust because SQLite lower() is ASCII only
ALTER TABLE notes ADD COLUMN title_key TEXT NOT NULL DEFAULT '';
CREATE INDEX notes_title_key ON notes (title_key);

-- one row per distinct link target of a note, target_key is the lowercased title
CREATE TABLE note_links (
    source_id   TEXT NOT NULL REFERENCES notes (id) ON DELETE CASCADE,
    target_key  TEXT NOT NULL,
    PRIMARY KEY (source_id, target_key)
) WITHOUT ROWID;
CREATE INDEX note_links_target ON note_links (target_key);

-- small values that belong to the vault, inside the encrypted database
CREATE TABLE vault_meta (
    key    TEXT PRIMARY KEY,
    value  BLOB NOT NULL
) WITHOUT ROWID;

-- attachment names are unique per note so `attachments/<name>` always resolves
CREATE UNIQUE INDEX attachments_note_name ON attachments (note_id, file_name);
