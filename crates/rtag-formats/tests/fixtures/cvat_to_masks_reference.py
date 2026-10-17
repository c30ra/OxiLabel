# Pixel counts of cvat_to_masks.draw_shape (the author's defect_detection/seg_prep script) for the
# polylines and point of tests/yolo_segment_obb.rs; the expected areas there come from its output:
#   polyline 50,80;150,80 width 4      -> 513 pixels, x 48..153, y 78..83
#   points 180,20 radius 3             -> 29 pixels,  x 177..184, y 17..24
#   polyline 20,60;60,90;100,60 width 7 -> 866 pixels, x 16..105, y 56..95
# Run with OpenCV and numpy: python reference.py path/to/cvat_to_masks.py
import sys, types, importlib.util
import xml.etree.ElementTree as ET
import numpy as np
for name in ("PIL", "common"):
    stub = types.ModuleType(name)
    stub.Image = None
    stub.load_classes = None
    sys.modules[name] = stub
spec = importlib.util.spec_from_file_location("cvat_to_masks", sys.argv[1])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
for tag, points, cls in [
    ("polyline", "50,80;150,80", {"line_width": 4}),
    ("points", "180,20", {"point_radius": 3}),
    ("polyline", "20,60;60,90;100,60", {"line_width": 7}),
]:
    canvas = np.zeros((100, 200), np.uint8)
    el = ET.Element(tag, points=points)
    module.draw_shape(canvas, el, 1, cls)
    ys, xs = np.nonzero(canvas)
    print(tag, points, cls, "pixels", int(canvas.sum()), "x", xs.min(), xs.max() + 1, "y", ys.min(), ys.max() + 1)
