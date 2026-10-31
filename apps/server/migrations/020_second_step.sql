-- SPDX-License-Identifier: AGPL-3.0-or-later
-- A second step is no longer forced. An account may take a code by email as
-- its second step, once its address is confirmed and the server sends mail;
-- the codes sent wait here, one per sign-in in progress, by the hash of that
-- sign-in's challenge. Whether admins must have a second step is the
-- setting 'admin_second_step' ('required' unless set to 'optional').

ALTER TABLE users ADD COLUMN email_codes INTEGER NOT NULL DEFAULT 0;

CREATE TABLE email_codes (
    challenge BLOB PRIMARY KEY,
    code BLOB NOT NULL,
    expires_at INTEGER NOT NULL,
    sends INTEGER NOT NULL DEFAULT 1
);
