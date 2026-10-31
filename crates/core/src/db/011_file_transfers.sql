-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Files the server refused for a space (too large, a type not allowed):
-- not sent again every round, and shown to the person.

CREATE TABLE files_refused (
    space   TEXT NOT NULL,
    sha256  TEXT NOT NULL,
    code    TEXT NOT NULL,
    at      INTEGER NOT NULL,
    PRIMARY KEY (space, sha256)
) WITHOUT ROWID;

-- the address a file has on the server for a space: an e2ee file's depends
-- on a key, and is needed to let it go after its last use is gone
ALTER TABLE files_on_server ADD COLUMN address TEXT;

-- where a moved note's files were before: a device that moves a note whose
-- file is not here yet fetches it from there (the space it left keeps it
-- for the retention time), then sends it to the new space
CREATE TABLE file_sources (
    sha256  TEXT NOT NULL,
    space   TEXT NOT NULL,
    epoch   INTEGER,
    PRIMARY KEY (sha256, space)
) WITHOUT ROWID;

-- a file attached here may have been let go by another device since this
-- one last looked: its next round asks the server again (and takes it back)
CREATE TRIGGER files_reclaim_insert AFTER INSERT ON attachments
WHEN NOT EXISTS (SELECT 1 FROM sync_quiet)
BEGIN
    DELETE FROM files_on_server
    WHERE space = (SELECT space_id FROM notes WHERE id = NEW.note_id)
      AND sha256 IN (NEW.sha256, NEW.thumb_sha256);
END;

CREATE TRIGGER files_reclaim_update AFTER UPDATE ON attachments
WHEN NOT EXISTS (SELECT 1 FROM sync_quiet) AND NEW.removed_at IS NULL
BEGIN
    DELETE FROM files_on_server
    WHERE space = (SELECT space_id FROM notes WHERE id = NEW.note_id)
      AND sha256 IN (NEW.sha256, NEW.thumb_sha256);
END;
