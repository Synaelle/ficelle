-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Where imported notes came from: importing the same export again skips
-- what is already in. Kept on this device only, never synced.

CREATE TABLE imports (
    importer    TEXT NOT NULL,
    source_key  TEXT NOT NULL,
    note_id     TEXT NOT NULL,
    at          INTEGER NOT NULL,
    PRIMARY KEY (importer, source_key)
) WITHOUT ROWID;
