-- SPDX-License-Identifier: AGPL-3.0-or-later
-- People who read the feedback inbox besides the admins. The reports
-- themselves are in feedback.db, which knows nothing of accounts. The
-- switches and the read-only token for `reports pull` are json in settings,
-- keys 'feedback' and 'feedback_token'.

CREATE TABLE feedback_readers (
    user_id  TEXT PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    added_at INTEGER NOT NULL
) STRICT;
