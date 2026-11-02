-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The merged text a conflict was found with: a choice made later is applied
-- on top of what was typed since, instead of over it.

ALTER TABLE note_conflicts ADD COLUMN merged_title TEXT;
ALTER TABLE note_conflicts ADD COLUMN merged_body TEXT;
