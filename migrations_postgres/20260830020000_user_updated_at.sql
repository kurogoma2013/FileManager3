ALTER TABLE users ADD COLUMN updated_at TEXT;

UPDATE users
SET updated_at = COALESCE(created_at, CURRENT_TIMESTAMP)
WHERE updated_at IS NULL;
