ALTER TABLE projects ADD COLUMN updated_at DATETIME;
UPDATE projects SET updated_at = COALESCE(created_at, CURRENT_TIMESTAMP) WHERE updated_at IS NULL;
