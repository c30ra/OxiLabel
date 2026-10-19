-- Server settings an admin changes on the Settings screen, as JSON by key (e.g. "worker").
CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
