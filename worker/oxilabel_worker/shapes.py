"""OxiLabel's shapes as the worker returns them, in image pixels (x right, y down), and a decoder for
the image bytes adapters receive. Angles are radians, turning clockwise on screen (like
`atan2(dy, dx)` in image coordinates)."""
import math


def point(x, y):
    return {"x": float(x), "y": float(y)}


def rectangle(x0, y0, x1, y1):
    return {"type": "rectangle", "min": point(min(x0, x1), min(y0, y1)), "max": point(max(x0, x1), max(y0, y1))}


def oriented_rectangle(cx, cy, width, height, angle):
    """A turned box: centre, the side along `angle`, the side across it."""
    return {"type": "oriented_rectangle", "centre": point(cx, cy), "width": float(width), "height": float(height), "angle": float(angle)}


def oriented_from_corners(corners):
    """The turned box through four corners in drawing order (as Ultralytics' OBB `xyxyxyxy`)."""
    (x0, y0), (x1, y1), (x2, y2), _ = [(float(x), float(y)) for x, y in corners]
    cx, cy = (x0 + x2) / 2, (y0 + y2) / 2
    return oriented_rectangle(cx, cy, math.hypot(x1 - x0, y1 - y0), math.hypot(x2 - x1, y2 - y1), math.atan2(y1 - y0, x1 - x0))


def polygon(points):
    return {"type": "polygon", "points": [point(x, y) for x, y in points]}


def polyline(points):
    return {"type": "polyline", "points": [point(x, y) for x, y in points]}


def points(points):
    return {"type": "points", "points": [point(x, y) for x, y in points]}


def ellipse(cx, cy, rx, ry, angle=0.0):
    return {"type": "ellipse", "centre": point(cx, cy), "rx": float(rx), "ry": float(ry), "angle": float(angle)}


def tag():
    """The class applies to the whole image."""
    return {"type": "tag"}


def mask(pixels):
    """A brush mask from a 2-D array the size of the image (nonzero = set); None if nothing is set.
    Stored as OxiLabel does: the rows of its bounding box, run-length encoded, starting unset."""
    import numpy as np

    set_rows = np.flatnonzero(pixels.any(axis=1))
    set_cols = np.flatnonzero(pixels.any(axis=0))
    if set_rows.size == 0:
        return None
    top, bottom, left, right = set_rows[0], set_rows[-1] + 1, set_cols[0], set_cols[-1] + 1
    flat = (pixels[top:bottom, left:right] != 0).ravel()
    # Runs start where the value changes; the first run is unset (empty if the first pixel is set).
    changes = np.flatnonzero(np.diff(flat.astype(np.int8))) + 1
    edges = np.concatenate(([0], changes, [flat.size]))
    runs = np.diff(edges).tolist()
    if flat[0]:
        runs.insert(0, 0)
    return {"type": "mask", "left": int(left), "top": int(top), "width": int(right - left), "height": int(bottom - top), "rle": [int(run) for run in runs]}


def decode(image):
    """The image file's bytes as an OpenCV image (BGR, or as stored for 16-bit and gray files)."""
    import cv2
    import numpy as np

    decoded = cv2.imdecode(np.frombuffer(image, np.uint8), cv2.IMREAD_UNCHANGED)
    if decoded is None:
        raise ValueError("the image cannot be decoded")
    return decoded
