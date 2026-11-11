-- Folders become nested tags: a tag's name is its path ("Projects/Garden"),
-- every level has a row of its own. The folders table and notes.folder_id
-- stay, an older device may still put notes in folders and the way back to
-- folders needs them. The conversion itself runs after this step, in Rust.

-- a tag kept without any note: made on its own, or from an empty folder
ALTER TABLE tags ADD COLUMN explicit INTEGER NOT NULL DEFAULT 0;
-- a muted colour mark, by name
ALTER TABLE tags ADD COLUMN color TEXT;
-- the folder a tag was made from, for the way back to folders
ALTER TABLE tags ADD COLUMN origin_folder TEXT;

-- explicit and coloured tags are shared through the person's own space
CREATE TRIGGER sync_tag_registry_insert AFTER INSERT ON tags
WHEN NOT EXISTS (SELECT 1 FROM sync_quiet) AND (NEW.explicit = 1 OR NEW.color IS NOT NULL)
BEGIN
    INSERT OR IGNORE INTO sync_touched (kind, space, id) VALUES ('tags', '', '*');
END;

CREATE TRIGGER sync_tag_registry_update AFTER UPDATE OF name, explicit, color ON tags
WHEN NOT EXISTS (SELECT 1 FROM sync_quiet)
    AND (NEW.explicit = 1 OR NEW.color IS NOT NULL OR OLD.explicit = 1 OR OLD.color IS NOT NULL)
BEGIN
    INSERT OR IGNORE INTO sync_touched (kind, space, id) VALUES ('tags', '', '*');
END;

CREATE TRIGGER sync_tag_registry_delete AFTER DELETE ON tags
WHEN NOT EXISTS (SELECT 1 FROM sync_quiet) AND (OLD.explicit = 1 OR OLD.color IS NOT NULL)
BEGIN
    INSERT OR IGNORE INTO sync_touched (kind, space, id) VALUES ('tags', '', '*');
END;
