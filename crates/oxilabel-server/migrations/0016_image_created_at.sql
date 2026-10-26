-- File creation time in Unix seconds; NULL where the file system does not record it.
ALTER TABLE images ADD COLUMN created_at INTEGER;
