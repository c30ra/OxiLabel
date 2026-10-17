-- How wide a label's polylines and how large its points become in exports that need areas.
ALTER TABLE labels ADD COLUMN line_width REAL NOT NULL DEFAULT 7;
ALTER TABLE labels ADD COLUMN point_radius REAL NOT NULL DEFAULT 8;
