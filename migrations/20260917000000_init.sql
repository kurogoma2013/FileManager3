-- FileManager3 PostgreSQL スキーマ
-- 日時はすべて TIMESTAMPTZ（UTC）で保存し、表示時に Asia/Tokyo へ変換する。

CREATE TABLE users (
    id BIGSERIAL PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'member',
    webauthn_id TEXT,
    active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE sessions (
    id BIGSERIAL PRIMARY KEY,
    token_hash TEXT NOT NULL UNIQUE,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE passkeys (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    credential_id TEXT NOT NULL UNIQUE,
    passkey_json TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE passkey_challenges (
    challenge_id TEXT PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    challenge_type TEXT NOT NULL,
    state_json TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE auth_login_attempts (
    username TEXT PRIMARY KEY,
    attempts INTEGER NOT NULL DEFAULT 0,
    window_started_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    blocked_until TIMESTAMPTZ
);

CREATE TABLE oauth_states (
    state TEXT PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE oauth_identities (
    id BIGSERIAL PRIMARY KEY,
    provider TEXT NOT NULL,
    subject TEXT NOT NULL,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    email TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(provider, subject)
);

CREATE TABLE dealers (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    kana TEXT NOT NULL DEFAULT '',
    address TEXT NOT NULL DEFAULT '',
    phone TEXT NOT NULL DEFAULT '',
    fax TEXT NOT NULL DEFAULT '',
    email TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at TIMESTAMPTZ
);

-- 担当者は案件が参照する販売店名に紐づける（販売店名の変更・削除に追従）
CREATE TABLE dealer_contacts (
    id BIGSERIAL PRIMARY KEY,
    dealer_name TEXT NOT NULL REFERENCES dealers(name) ON UPDATE CASCADE ON DELETE CASCADE,
    name TEXT NOT NULL,
    phone TEXT NOT NULL DEFAULT '',
    email TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at TIMESTAMPTZ
);

CREATE TABLE projects (
    id BIGSERIAL PRIMARY KEY,
    project_number TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    kana TEXT NOT NULL DEFAULT '',
    address TEXT NOT NULL DEFAULT '',
    dealer TEXT,
    assignee TEXT,
    phone TEXT,
    email TEXT NOT NULL DEFAULT '',
    latitude DOUBLE PRECISION,
    longitude DOUBLE PRECISION,
    plus_code TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at TIMESTAMPTZ
);

CREATE TABLE files (
    id BIGSERIAL PRIMARY KEY,
    project_id BIGINT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    file_name TEXT NOT NULL,
    storage_path TEXT NOT NULL,
    file_type TEXT NOT NULL,
    version_number INTEGER NOT NULL DEFAULT 1,
    tag TEXT NOT NULL DEFAULT '',
    file_size BIGINT NOT NULL DEFAULT 0,
    file_hash TEXT NOT NULL,
    source_hash TEXT NOT NULL,
    uploaded_by BIGINT REFERENCES users(id) ON DELETE SET NULL,
    deleted_by BIGINT REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at TIMESTAMPTZ
);

CREATE TABLE file_histories (
    id BIGSERIAL PRIMARY KEY,
    file_id BIGINT NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    project_id BIGINT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    version_number INTEGER NOT NULL,
    file_name TEXT NOT NULL,
    storage_path TEXT NOT NULL,
    file_type TEXT NOT NULL,
    file_hash TEXT NOT NULL,
    source_hash TEXT NOT NULL,
    file_size BIGINT NOT NULL DEFAULT 0,
    tag TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    archived_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE file_search (
    file_id BIGINT PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
    project_id BIGINT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    filename TEXT NOT NULL,
    content TEXT NOT NULL
);

CREATE TABLE project_notes (
    id BIGSERIAL PRIMARY KEY,
    project_id BIGINT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    content TEXT NOT NULL,
    created_by TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at TIMESTAMPTZ
);

CREATE TABLE dealer_notes (
    id BIGSERIAL PRIMARY KEY,
    dealer_id BIGINT NOT NULL REFERENCES dealers(id) ON DELETE CASCADE,
    content TEXT NOT NULL,
    created_by TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at TIMESTAMPTZ
);

CREATE TABLE project_permissions (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    project_id BIGINT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    permission TEXT NOT NULL DEFAULT 'member',
    UNIQUE(user_id, project_id)
);

CREATE INDEX idx_sessions_expiry ON sessions(expires_at);
CREATE INDEX idx_passkey_challenges_expiry ON passkey_challenges(expires_at);
CREATE INDEX idx_auth_login_attempts_blocked_until ON auth_login_attempts(blocked_until);
CREATE INDEX idx_oauth_states_created_at ON oauth_states(created_at);
CREATE INDEX idx_oauth_identities_user_id ON oauth_identities(user_id);
CREATE INDEX idx_projects_deleted_at ON projects(deleted_at);
CREATE INDEX idx_projects_dealer_active ON projects(dealer, deleted_at);
CREATE INDEX idx_dealers_deleted_at ON dealers(deleted_at);
CREATE INDEX idx_dealer_contacts_name_active ON dealer_contacts(dealer_name, name, deleted_at);
CREATE INDEX idx_files_project_active ON files(project_id, deleted_at);
CREATE INDEX idx_files_project_type ON files(project_id, deleted_at, file_type);
CREATE INDEX idx_files_hash ON files(file_hash);
-- 物理ファイルは案件ごとに共有するため file_hash は一意にせず、共有参照と有効行の検索に使う
CREATE INDEX idx_files_hash_active ON files(file_hash, deleted_at);
CREATE INDEX idx_files_source_hash ON files(project_id, source_hash);
CREATE INDEX idx_files_uploaded_by ON files(uploaded_by);
CREATE INDEX idx_files_deleted_by ON files(deleted_by);
CREATE INDEX idx_file_histories_file_id ON file_histories(file_id);
CREATE INDEX idx_file_histories_project ON file_histories(project_id);
CREATE INDEX idx_file_search_fts ON file_search
    USING GIN (to_tsvector('simple', coalesce(filename, '') || ' ' || coalesce(content, '')));
CREATE INDEX idx_project_notes_project_id ON project_notes(project_id);
CREATE INDEX idx_dealer_notes_dealer_id ON dealer_notes(dealer_id);
