CREATE TABLE auth_sessions (
    id TEXT PRIMARY KEY NOT NULL,
    user_id INTEGER NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    revoked_at INTEGER,
    FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX auth_sessions_user_active
    ON auth_sessions(user_id, expires_at)
    WHERE revoked_at IS NULL;
