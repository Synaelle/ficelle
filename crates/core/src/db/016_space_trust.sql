-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Who the owner of an e2ee space let in from this vault, with the public
-- key they had then: space keys are sealed for these members only, never
-- for whoever the server lists. A person chosen here before they had a key
-- has none yet, the first one seen is kept.

CREATE TABLE space_trust (
  space_id TEXT NOT NULL,
  user_id TEXT NOT NULL,
  public_key TEXT,
  PRIMARY KEY (space_id, user_id)
);

-- Members the server lists as waiting for a key that the owner has not let
-- in, or whose key is not the one kept: the share dialog asks about them.
CREATE TABLE space_waiting (
  space_id TEXT NOT NULL,
  user_id TEXT NOT NULL,
  name TEXT NOT NULL DEFAULT '',
  public_key TEXT NOT NULL,
  changed INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (space_id, user_id)
);
