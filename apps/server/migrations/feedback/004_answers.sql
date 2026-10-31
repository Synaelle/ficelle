-- SPDX-License-Identifier: AGPL-3.0-or-later
-- A report's receipt, the hash of a secret only the sending app keeps, so
-- that app alone can ask how its report stands; and the one answer the
-- moderators give, which it then shows. The note stays the moderators' own.

ALTER TABLE reports ADD COLUMN receipt TEXT;
ALTER TABLE reports ADD COLUMN answer TEXT NOT NULL DEFAULT '';

CREATE INDEX reports_by_receipt ON reports (receipt);
