-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Every report goes after the instance's keeping time, none is kept longer.

ALTER TABLE reports DROP COLUMN kept;
