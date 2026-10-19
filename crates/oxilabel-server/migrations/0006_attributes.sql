-- Attribute definitions of a label (JSON list) and an annotation's attribute values (JSON
-- object by name).
ALTER TABLE labels ADD COLUMN attributes TEXT NOT NULL DEFAULT '[]';
ALTER TABLE annotations ADD COLUMN attributes TEXT NOT NULL DEFAULT '{}';
