-- Hidden (not drawn or picked in the editor) and locked (not moved or deleted) annotations.
ALTER TABLE annotations ADD COLUMN hidden INTEGER NOT NULL DEFAULT 0;
ALTER TABLE annotations ADD COLUMN locked INTEGER NOT NULL DEFAULT 0;
