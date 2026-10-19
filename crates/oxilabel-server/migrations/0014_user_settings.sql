-- Each user's own settings (shortcuts, pie flick distance, label style, theme) as JSON.
CREATE TABLE user_settings (
    user_id INTEGER PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    value TEXT NOT NULL
);
