-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Since 0.2.0 a report says what it is about in one line instead of a
-- category (left empty), and the answer goes to an email address or a
-- Discord name. A block on a contact keeps the kind 'email'.

ALTER TABLE reports ADD COLUMN about TEXT;
ALTER TABLE reports RENAME COLUMN email TO contact;
