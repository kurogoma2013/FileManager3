CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'member',
    webauthn_id TEXT,
    active INTEGER NOT NULL DEFAULT 1,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE sessions (
    id INTEGER PRIMARY KEY,
    token_hash TEXT NOT NULL UNIQUE,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at DATETIME NOT NULL
);

CREATE TABLE passkeys (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    credential_id TEXT NOT NULL UNIQUE,
    passkey_json TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE passkey_challenges (
    challenge_id TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    challenge_type TEXT NOT NULL,
    state_json TEXT NOT NULL,
    expires_at DATETIME NOT NULL
);

CREATE TABLE projects (
    id INTEGER PRIMARY KEY,
    project_number TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    kana TEXT NOT NULL DEFAULT '',
    address TEXT NOT NULL DEFAULT '',
    dealer TEXT,
    assignee TEXT,
    phone TEXT,
    latitude REAL,
    longitude REAL,
    plus_code TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at DATETIME
);

CREATE TABLE dealers (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    kana TEXT NOT NULL DEFAULT '',
    address TEXT NOT NULL DEFAULT '',
    phone TEXT NOT NULL DEFAULT '',
    fax TEXT NOT NULL DEFAULT '',
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at DATETIME
);

CREATE TABLE dealer_contacts (
    id INTEGER PRIMARY KEY,
    dealer_name TEXT NOT NULL,
    name TEXT NOT NULL,
    phone TEXT NOT NULL DEFAULT '',
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at DATETIME
);

CREATE TABLE files (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    file_name TEXT NOT NULL,
    storage_path TEXT NOT NULL,
    file_type TEXT NOT NULL,
    version_number INTEGER NOT NULL DEFAULT 1,
    tag TEXT NOT NULL DEFAULT '',
    file_size INTEGER NOT NULL DEFAULT 0,
    file_hash TEXT NOT NULL,
    source_hash TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at DATETIME
);

CREATE TABLE file_histories (
    id INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    version_number INTEGER NOT NULL,
    file_name TEXT NOT NULL,
    storage_path TEXT NOT NULL,
    file_type TEXT NOT NULL,
    file_hash TEXT NOT NULL,
    source_hash TEXT NOT NULL,
    file_size INTEGER NOT NULL DEFAULT 0,
    tag TEXT NOT NULL DEFAULT '',
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    archived_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE project_notes (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    content TEXT NOT NULL,
    created_by TEXT NOT NULL DEFAULT '',
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at DATETIME
);

CREATE TABLE dealer_notes (
    id INTEGER PRIMARY KEY,
    dealer_id INTEGER NOT NULL REFERENCES dealers(id) ON DELETE CASCADE,
    content TEXT NOT NULL,
    created_by TEXT NOT NULL DEFAULT '',
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at DATETIME
);

CREATE TABLE project_permissions (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    permission TEXT NOT NULL DEFAULT 'member',
    UNIQUE(user_id, project_id)
);

CREATE VIRTUAL TABLE file_search USING fts5(
    file_id UNINDEXED,
    project_id UNINDEXED,
    filename,
    content,
    tokenize = 'unicode61'
);

CREATE INDEX idx_sessions_expiry ON sessions(expires_at);
CREATE INDEX idx_passkey_challenges_expiry ON passkey_challenges(expires_at);
CREATE INDEX idx_projects_deleted_at ON projects(deleted_at);
CREATE INDEX idx_projects_dealer_active ON projects(dealer, deleted_at);
CREATE INDEX idx_dealers_deleted_at ON dealers(deleted_at);
CREATE INDEX idx_dealer_contacts_name_active ON dealer_contacts(dealer_name, name, deleted_at);
CREATE INDEX idx_files_project_active ON files(project_id, deleted_at);
CREATE INDEX idx_files_project_type ON files(project_id, deleted_at, file_type);
CREATE INDEX idx_files_hash ON files(file_hash);
CREATE INDEX idx_files_source_hash ON files(project_id, source_hash);
CREATE INDEX idx_file_histories_file_id ON file_histories(file_id);
CREATE INDEX idx_file_histories_project ON file_histories(project_id);
CREATE INDEX idx_project_notes_project_id ON project_notes(project_id);
CREATE INDEX idx_dealer_notes_dealer_id ON dealer_notes(dealer_id);

CREATE TRIGGER file_search_soft_delete
AFTER UPDATE OF deleted_at ON files
WHEN new.deleted_at IS NOT NULL
BEGIN
    DELETE FROM file_search WHERE file_id = new.id;
END;

CREATE TRIGGER file_search_delete
AFTER DELETE ON files
BEGIN
    DELETE FROM file_search WHERE file_id = old.id;
END;
