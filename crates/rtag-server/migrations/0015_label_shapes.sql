-- A label's point footprint shape (disc or square), whether the editor shows its footprint, and
-- its shape constraints (fixed sizes, JSON object by shape type).
ALTER TABLE labels ADD COLUMN point_shape TEXT NOT NULL DEFAULT 'disc';
ALTER TABLE labels ADD COLUMN show_footprint INTEGER NOT NULL DEFAULT 1;
ALTER TABLE labels ADD COLUMN constraints TEXT NOT NULL DEFAULT '{}';
