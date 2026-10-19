# Reference outputs of the enhancement filters in the author's defect_detection/seg_prep/common.py
# (flatten_illumination, enhance_for_tagging, enhance_v2) for tests/filters.rs. Draws a synthetic
# electroluminescence-like photo (uneven lighting, cell grid, fingers, dark spots, a hot spot,
# noise; a width no tile or block divides, a height 16 tiles do), then writes it and each filter's output as PNG
# here. Run with OpenCV and numpy: python reference.py path/to/common.py
import sys, types, importlib.util
from pathlib import Path
import cv2
import numpy as np
sys.modules["yaml"] = types.ModuleType("yaml")
spec = importlib.util.spec_from_file_location("common", sys.argv[1])
common = importlib.util.module_from_spec(spec)
spec.loader.exec_module(common)
out = Path(__file__).parent

h, w = 224, 333
rng = np.random.default_rng(7)
y, x = np.mgrid[0:h, 0:w].astype(np.float64)
light = 150 + 70 * np.exp(-(((x - 120) / 260) ** 2 + ((y - 90) / 180) ** 2)) - 0.12 * x
plate = light.copy()
plate[(x.astype(int) % 41) < 2] *= 0.35          # cell borders
plate[(y.astype(int) % 37) < 2] *= 0.35
plate[(y.astype(int) % 6) == 3] *= 0.86          # fingers
for cx, cy, r, k in [(60, 50, 6, 0.4), (200, 140, 9, 0.6), (290, 40, 3, 0.3), (150, 190, 14, 0.75)]:
    plate[(x - cx) ** 2 + (y - cy) ** 2 < r * r] *= k
plate += 120 * np.exp(-((x - 250) ** 2 + (y - 170) ** 2) / 50)   # hot spot
plate += rng.normal(0, 6, plate.shape)
bgr = np.stack([plate * 0.92, plate, plate * 1.05 + 4], axis=2)
image = np.clip(bgr, 0, 255).astype(np.uint8)
cv2.imwrite(str(out / "input.png"), image)

outputs = {
    "flatten.png": common.flatten_illumination(image),
    "flatten_wide.png": common.flatten_illumination(image, sigma_frac=0.45),
    "clahe.png": common.enhance_for_tagging(image),
    "clahe_options.png": common.enhance_for_tagging(image, sigma_frac=0.1, clip=1.5, tiles=5),
    "v2.png": common.enhance_v2(image),
    "v2_options.png": common.enhance_v2(image, bg_frac=0.08, detail_sigma=3.0, norm_frac=0.05, gain=1.2, clip=2.0),
}
for name, result in outputs.items():
    cv2.imwrite(str(out / name), result)
    print(name, result.shape, result.dtype, round(float(result.mean()), 2))
