-- SPDX-License-Identifier: AGPL-3.0-or-later
-- First schema. Ids are uuid v7 text, times are unix milliseconds.

CREATE TABLE folders (
    id          TEXT PRIMARY KEY,
    parent_id   TEXT REFERENCES folders (id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    position    INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    trashed_at  INTEGER
);
CREATE INDEX folders_parent ON folders (parent_id, position);

-- notes keep the implicit rowid because the search index is keyed by it
CREATE TABLE notes (
    id          TEXT NOT NULL UNIQUE,
    folder_id   TEXT REFERENCES folders (id) ON DELETE SET NULL,
    title       TEXT NOT NULL DEFAULT '',
    body        TEXT NOT NULL DEFAULT '',
    pinned      INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    trashed_at  INTEGER
);
CREATE INDEX notes_folder ON notes (folder_id, trashed_at);
CREATE INDEX notes_updated ON notes (updated_at);

CREATE TABLE tags (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE COLLATE NOCASE,
    created_at  INTEGER NOT NULL
);

CREATE TABLE note_tags (
    note_id     TEXT NOT NULL REFERENCES notes (id) ON DELETE CASCADE,
    tag_id      TEXT NOT NULL REFERENCES tags (id) ON DELETE CASCADE,
    PRIMARY KEY (note_id, tag_id)
) WITHOUT ROWID;
CREATE INDEX note_tags_tag ON note_tags (tag_id);

CREATE TABLE note_versions (
    id          TEXT PRIMARY KEY,
    note_id     TEXT NOT NULL REFERENCES notes (id) ON DELETE CASCADE,
    title       TEXT NOT NULL,
    body        TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    reason      TEXT NOT NULL
);
CREATE INDEX note_versions_note ON note_versions (note_id, created_at);

CREATE TABLE attachments (
    id          TEXT PRIMARY KEY,
    note_id     TEXT NOT NULL REFERENCES notes (id) ON DELETE CASCADE,
    file_name   TEXT NOT NULL,
    mime_type   TEXT NOT NULL,
    size        INTEGER NOT NULL,
    sha256      TEXT NOT NULL,
    created_at  INTEGER NOT NULL
);
CREATE INDEX attachments_note ON attachments (note_id);

-- full text search over title, body and tag names, rowid = notes.rowid.
-- unicode61 with diacritics removed so "ete" finds "été".
CREATE VIRTUAL TABLE notes_fts USING fts5 (
    title,
    body,
    tags,
    tokenize = 'unicode61 remove_diacritics 2'
);
