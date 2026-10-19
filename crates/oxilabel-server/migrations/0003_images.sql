CREATE TABLE images (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    hash TEXT NOT NULL,
    width INTEGER NOT NULL,
    height INTEGER NOT NULL,
    annotated INTEGER NOT NULL DEFAULT 0,
    UNIQUE (project_id, path)
);

CREATE INDEX images_by_hash ON images (project_id, hash);
