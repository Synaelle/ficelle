-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Failed sign-ins, counted for an account from one network, so someone else
-- cannot lock a person out from theirs, and for the account from all of them
-- (network ''), many more allowed. A username no account has is counted the
-- same way by a hash of the name, so a lock tells nothing about which names
-- exist. The users table's own counters are no longer used.

CREATE TABLE sign_in_locks (
    subject TEXT NOT NULL,
    network TEXT NOT NULL,
    failed INTEGER NOT NULL,
    lock_count INTEGER NOT NULL,
    locked_until INTEGER,
    last_at INTEGER NOT NULL,
    PRIMARY KEY (subject, network)
) STRICT;
