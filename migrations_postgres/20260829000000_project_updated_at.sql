ALTER TABLE projects ADD COLUMN updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP::text;
UPDATE projects SET updated_at = COALESCE(created_at, CURRENT_TIMESTAMP::text) WHERE updated_at IS NULL;
