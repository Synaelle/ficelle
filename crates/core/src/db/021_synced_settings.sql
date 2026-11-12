-- The person's choices that follow them to every device (theme, sort,
-- typing helpers...), each a JSON value under the name the app gives it,
-- with when it was chosen: the latest choice wins on every device. Only a
-- choice made on some device has a row: defaults never travel, so a new
-- device does not undo what the others chose.
CREATE TABLE synced_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    changed_at INTEGER NOT NULL
) WITHOUT ROWID;

-- shared through the person's own space
CREATE TRIGGER sync_settings_insert AFTER INSERT ON synced_settings
WHEN NOT EXISTS (SELECT 1 FROM sync_quiet)
BEGIN
    INSERT OR IGNORE INTO sync_touched (kind, space, id) VALUES ('settings', '', '*');
END;

CREATE TRIGGER sync_settings_update AFTER UPDATE ON synced_settings
WHEN NOT EXISTS (SELECT 1 FROM sync_quiet)
BEGIN
    INSERT OR IGNORE INTO sync_touched (kind, space, id) VALUES ('settings', '', '*');
END;

CREATE TRIGGER sync_settings_delete AFTER DELETE ON synced_settings
WHEN NOT EXISTS (SELECT 1 FROM sync_quiet)
BEGIN
    INSERT OR IGNORE INTO sync_touched (kind, space, id) VALUES ('settings', '', '*');
END;
