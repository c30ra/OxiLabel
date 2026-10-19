-- Local accounts and their login sessions.
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    username TEXT NOT NULL UNIQUE COLLATE NOCASE,
    -- argon2 in PHC string format
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('admin', 'annotator'))
);

-- Only a hash of each session token is stored, so the database alone cannot sign anyone in.
CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- Unix seconds
    expires_at INTEGER NOT NULL
);
CREATE INDEX sessions_user ON sessions (user_id);
