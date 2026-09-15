CREATE TABLE auth_login_attempts (
    username TEXT PRIMARY KEY,
    attempts INTEGER NOT NULL DEFAULT 0,
    window_started_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    blocked_until TEXT
);

CREATE INDEX idx_auth_login_attempts_blocked_until
    ON auth_login_attempts(blocked_until);
