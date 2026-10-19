-- Circles become ellipses with equal radii (ticket 23).
UPDATE annotations
SET shape = json_object(
    'type', 'ellipse',
    'centre', json_extract(shape, '$.centre'),
    'rx', json_extract(shape, '$.radius'),
    'ry', json_extract(shape, '$.radius'),
    'angle', 0.0)
WHERE json_extract(shape, '$.type') = 'circle';
UPDATE labels SET shape_types = replace(shape_types, '"circle"', '"ellipse"');
