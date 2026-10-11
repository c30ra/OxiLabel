-- File size in bytes and modification time (Unix seconds) as found at import.
-- Rows imported before this migration are filled in at startup.
ALTER TABLE images ADD COLUMN size INTEGER;
ALTER TABLE images ADD COLUMN modified_at INTEGER;
