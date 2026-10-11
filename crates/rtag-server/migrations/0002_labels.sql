ALTER TABLE projects ADD COLUMN next_label_id INTEGER NOT NULL DEFAULT 1;

CREATE TABLE labels (
    project_id INTEGER NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    id INTEGER NOT NULL,
    name TEXT NOT NULL,
    color TEXT NOT NULL,
    hotkey TEXT,
    shape_types TEXT NOT NULL,
    is_ignore INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (project_id, id)
);
