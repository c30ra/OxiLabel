-- Pre-annotation (ticket 17): models the worker runs, and unreviewed pre-annotations.
ALTER TABLE annotations ADD COLUMN pending INTEGER NOT NULL DEFAULT 0;

CREATE TABLE models (
    id INTEGER PRIMARY KEY,
    -- Also the source of its pre-annotations.
    name TEXT NOT NULL UNIQUE COLLATE NOCASE,
    adapter TEXT NOT NULL,
    weights TEXT NOT NULL,
    -- JSON objects: adapter options, and label names by class name.
    options TEXT NOT NULL DEFAULT '{}',
    class_map TEXT NOT NULL DEFAULT '{}'
);
