-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Notes made on this device. The cleanup at startup removes only these when
-- they were left blank: a blank note that came from another device may be
-- one someone there is about to write in. Notes from before are counted as
-- made here, as the cleanup treated them until now.

CREATE TABLE notes_made_here (
    note_id TEXT PRIMARY KEY REFERENCES notes (id) ON DELETE CASCADE
) WITHOUT ROWID;

INSERT INTO notes_made_here (note_id) SELECT id FROM notes;

CREATE TRIGGER notes_made_here AFTER INSERT ON notes
WHEN NOT EXISTS (SELECT 1 FROM sync_quiet)
BEGIN
    INSERT OR IGNORE INTO notes_made_here (note_id) VALUES (NEW.id);
END;
