-- SPDX-License-Identifier: AGPL-3.0-or-later
-- When an item trashed on another device reached this one. Trash retention
-- counts from then: a device whose clock is far behind dates its trash in
-- the past, and this device would otherwise delete those items for good the
-- next time it starts, before anyone here saw them in the trash.

CREATE TABLE trash_arrived (
    kind    TEXT NOT NULL,
    id      TEXT NOT NULL,
    at      INTEGER NOT NULL,
    PRIMARY KEY (kind, id)
) WITHOUT ROWID;
