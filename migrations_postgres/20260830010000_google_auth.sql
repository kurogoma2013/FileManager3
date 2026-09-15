CREATE TABLE oauth_states (
    state TEXT PRIMARY KEY,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE oauth_identities (
    id BIGSERIAL PRIMARY KEY,
    provider TEXT NOT NULL,
    subject TEXT NOT NULL,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    email TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(provider, subject)
);

CREATE INDEX idx_oauth_states_created_at ON oauth_states(created_at);
CREATE INDEX idx_oauth_identities_user_id ON oauth_identities(user_id);
