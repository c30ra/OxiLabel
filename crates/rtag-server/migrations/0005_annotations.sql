-- Annotations of an image are saved together as one batch; every save increases the image's
-- version, and a save based on an older version is rejected.
ALTER TABLE images ADD COLUMN version INTEGER NOT NULL DEFAULT 0;

CREATE TABLE annotations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    image_id INTEGER NOT NULL REFERENCES images (id) ON DELETE CASCADE,
    project_id INTEGER NOT NULL,
    label_id INTEGER NOT NULL,
    -- Drawing order within the image; the highest is on top.
    position INTEGER NOT NULL,
    -- The shape as JSON in original-image coordinates, e.g. {"type":"rectangle","min":…,"max":…}.
    shape TEXT NOT NULL,
    source TEXT,
    confidence REAL,
    FOREIGN KEY (project_id, label_id) REFERENCES labels (project_id, id) ON DELETE CASCADE
);

CREATE INDEX annotations_by_image ON annotations (image_id, position);
CREATE INDEX annotations_by_label ON annotations (project_id, label_id);
