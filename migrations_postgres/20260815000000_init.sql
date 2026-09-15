-- PostgreSQL compatibility helpers for the application's legacy timestamp format.
CREATE OR REPLACE FUNCTION datetime(value TEXT, modifier TEXT DEFAULT NULL)
RETURNS TEXT
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    base_ts TIMESTAMP;
    amount INTEGER;
BEGIN
    base_ts := CASE WHEN value = 'now' THEN CURRENT_TIMESTAMP::timestamp ELSE value::timestamp END;
    IF modifier IS NULL THEN
        RETURN base_ts::text;
    END IF;
    amount := regexp_replace(modifier, '[^0-9-]', '', 'g')::integer;
    IF modifier LIKE '%hour%' THEN
        base_ts := base_ts + amount * INTERVAL '1 hour';
    ELSIF modifier LIKE '%minute%' THEN
        base_ts := base_ts + amount * INTERVAL '1 minute';
    ELSIF modifier LIKE '%second%' THEN
        base_ts := base_ts + amount * INTERVAL '1 second';
    ELSE
        RAISE EXCEPTION 'unsupported datetime modifier: %', modifier;
    END IF;
    RETURN base_ts::text;
END;
$$;

CREATE OR REPLACE FUNCTION strftime(format TEXT, value TEXT, modifier TEXT DEFAULT NULL)
RETURNS TEXT
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    timestamp_value TIMESTAMP := datetime(value, modifier)::timestamp;
BEGIN
    IF format LIKE '%T%' THEN
        RETURN to_char(timestamp_value, 'YYYY-MM-DD"T"HH24:MI:SS') || '+09:00';
    END IF;
    RETURN to_char(timestamp_value, 'YYYY-MM-DD HH24:MI:SS');
END;
$$;

CREATE TABLE users (id BIGSERIAL PRIMARY KEY, username TEXT NOT NULL UNIQUE, password_hash TEXT NOT NULL, role TEXT NOT NULL DEFAULT 'member', webauthn_id TEXT, active INTEGER NOT NULL DEFAULT 1, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP::text);
CREATE TABLE sessions (id BIGSERIAL PRIMARY KEY, token_hash TEXT NOT NULL UNIQUE, user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP::text, expires_at TEXT NOT NULL);
CREATE TABLE passkeys (id BIGSERIAL PRIMARY KEY, user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE, credential_id TEXT NOT NULL UNIQUE, passkey_json TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP::text);
CREATE TABLE passkey_challenges (challenge_id TEXT PRIMARY KEY, user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE, challenge_type TEXT NOT NULL, state_json TEXT NOT NULL, expires_at TEXT NOT NULL);
CREATE TABLE projects (id BIGSERIAL PRIMARY KEY, project_number TEXT NOT NULL UNIQUE, name TEXT NOT NULL, kana TEXT NOT NULL DEFAULT '', address TEXT NOT NULL DEFAULT '', dealer TEXT, assignee TEXT, phone TEXT, latitude DOUBLE PRECISION, longitude DOUBLE PRECISION, plus_code TEXT, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP::text, deleted_at TEXT);
CREATE TABLE dealers (id BIGSERIAL PRIMARY KEY, name TEXT NOT NULL UNIQUE, kana TEXT NOT NULL DEFAULT '', address TEXT NOT NULL DEFAULT '', phone TEXT NOT NULL DEFAULT '', fax TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP::text, deleted_at TEXT);
CREATE TABLE dealer_contacts (id BIGSERIAL PRIMARY KEY, dealer_name TEXT NOT NULL REFERENCES dealers(name) ON UPDATE CASCADE ON DELETE CASCADE, name TEXT NOT NULL, phone TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP::text, deleted_at TEXT);
CREATE TABLE files (id BIGSERIAL PRIMARY KEY, project_id BIGINT NOT NULL REFERENCES projects(id) ON DELETE CASCADE, file_name TEXT NOT NULL, storage_path TEXT NOT NULL, file_type TEXT NOT NULL, version_number INTEGER NOT NULL DEFAULT 1, tag TEXT NOT NULL DEFAULT '', file_size BIGINT NOT NULL DEFAULT 0, file_hash TEXT NOT NULL, source_hash TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP::text, deleted_at TEXT);
CREATE TABLE file_histories (id BIGSERIAL PRIMARY KEY, file_id BIGINT NOT NULL REFERENCES files(id) ON DELETE CASCADE, project_id BIGINT NOT NULL REFERENCES projects(id) ON DELETE CASCADE, version_number INTEGER NOT NULL, file_name TEXT NOT NULL, storage_path TEXT NOT NULL, file_type TEXT NOT NULL, file_hash TEXT NOT NULL, source_hash TEXT NOT NULL, file_size BIGINT NOT NULL DEFAULT 0, tag TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP::text, archived_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP::text);
CREATE TABLE project_notes (id BIGSERIAL PRIMARY KEY, project_id BIGINT NOT NULL REFERENCES projects(id) ON DELETE CASCADE, content TEXT NOT NULL, created_by TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP::text, deleted_at TEXT);
CREATE TABLE dealer_notes (id BIGSERIAL PRIMARY KEY, dealer_id BIGINT NOT NULL REFERENCES dealers(id) ON DELETE CASCADE, content TEXT NOT NULL, created_by TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP::text, deleted_at TEXT);
CREATE TABLE project_permissions (id BIGSERIAL PRIMARY KEY, user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE, project_id BIGINT NOT NULL REFERENCES projects(id) ON DELETE CASCADE, permission TEXT NOT NULL DEFAULT 'member', UNIQUE(user_id, project_id));
CREATE TABLE file_search (file_id BIGINT PRIMARY KEY, project_id BIGINT NOT NULL, filename TEXT NOT NULL, content TEXT NOT NULL);
CREATE INDEX idx_sessions_expiry ON sessions(expires_at); CREATE INDEX idx_passkey_challenges_expiry ON passkey_challenges(expires_at); CREATE INDEX idx_projects_deleted_at ON projects(deleted_at); CREATE INDEX idx_projects_dealer_active ON projects(dealer, deleted_at); CREATE INDEX idx_dealers_deleted_at ON dealers(deleted_at); CREATE INDEX idx_dealer_contacts_name_active ON dealer_contacts(dealer_name, name, deleted_at); CREATE INDEX idx_files_project_active ON files(project_id, deleted_at); CREATE INDEX idx_files_project_type ON files(project_id, deleted_at, file_type); CREATE INDEX idx_files_hash ON files(file_hash); CREATE INDEX idx_files_source_hash ON files(project_id, source_hash); CREATE INDEX idx_file_histories_file_id ON file_histories(file_id); CREATE INDEX idx_file_histories_project ON file_histories(project_id); CREATE INDEX idx_project_notes_project_id ON project_notes(project_id); CREATE INDEX idx_dealer_notes_dealer_id ON dealer_notes(dealer_id);
