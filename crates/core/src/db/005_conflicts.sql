-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Notes waiting for a decision after a sync: a suspicious merge (both sides
-- kept here and in the history), or a note edited elsewhere while it was
-- deleted here. Local to the device, each device that sees it asks.

CREATE TABLE note_conflicts (
    note_id         TEXT PRIMARY KEY REFERENCES notes (id) ON DELETE CASCADE,
    -- merge or editedAfterDelete
    kind            TEXT NOT NULL,
    mine_title      TEXT,
    mine_body       TEXT,
    theirs_title    TEXT,
    theirs_body     TEXT,
    at              INTEGER NOT NULL
) WITHOUT ROWID;
