"""The worker's HTTP contract and shape helpers, with a fake adapter (no PyTorch needed).

    cd worker && python3 -m unittest discover tests
"""
import base64
import json
import math
import os
import threading
import unittest
import urllib.error
import urllib.request
from pathlib import Path

os.environ["RTAG_WORKER_QUIET"] = "1"

from rtag_worker import shapes  # noqa: E402
from rtag_worker.server import Models, serve  # noqa: E402

ADAPTER = str(Path(__file__).with_name("fake_adapter.py"))


class WorkerTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server = serve("127.0.0.1:0")
        cls.base = f"http://127.0.0.1:{cls.server.server_address[1]}"
        threading.Thread(target=cls.server.serve_forever, daemon=True).start()

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()

    def call(self, path, body=None):
        data = None if body is None else json.dumps(body).encode()
        request = urllib.request.Request(self.base + path, data=data, headers={"Content-Type": "application/json"})
        try:
            with urllib.request.urlopen(request) as response:
                return response.status, json.load(response)
        except urllib.error.HTTPError as err:
            return err.code, json.load(err)

    def model(self, **extra):
        return {"adapter": ADAPTER, "weights": "fake.pt", "options": {"conf": 0.75}, **extra}

    def test_health_names_the_device(self):
        status, body = self.call("/health")
        self.assertEqual(status, 200)
        self.assertIsInstance(body["device"], str)

    def test_a_file_adapter_gives_classes_and_predictions(self):
        self.assertEqual(self.call("/classes", self.model()), (200, {"classes": ["crack", "chip"]}))
        image = base64.b64encode(b"12345").decode()
        status, body = self.call("/predict", self.model(image=image))
        self.assertEqual(status, 200)
        self.assertEqual(body["predictions"], [{"class": "crack", "confidence": 0.75, "shape": {"type": "rectangle", "min": {"x": 0.0, "y": 0.0}, "max": {"x": 5.0, "y": 1.0}}}])

    def test_a_model_is_loaded_once_per_weights_and_options(self):
        models = Models()
        first = models.get(self.model(weights="once.pt"))
        self.assertIs(models.get(self.model(weights="once.pt"))[1], first[1])
        other = models.get(self.model(weights="once.pt", options={"conf": 0.1}))
        self.assertIsNot(other[1], first[1])
        adapter = first[0]
        self.assertEqual([entry for entry in adapter.loads if entry[0] == "once.pt"], [("once.pt", {"conf": 0.75}), ("once.pt", {"conf": 0.1})])

    def test_at_most_two_models_stay_loaded_and_changed_weights_are_reloaded(self):
        import tempfile

        models = Models()
        for weights in ("a.pt", "b.pt", "c.pt"):
            models.get(self.model(weights=weights))
        self.assertEqual([key[1] for key in models.loaded], ["b.pt", "c.pt"])
        with tempfile.NamedTemporaryFile(suffix=".pt") as file:
            first = models.get(self.model(weights=file.name))
            self.assertIs(models.get(self.model(weights=file.name))[1], first[1])
            os.utime(file.name, ns=(1, 1))
            self.assertIsNot(models.get(self.model(weights=file.name))[1], first[1])

    def test_bad_requests_and_failing_adapters_are_answered_with_their_reason(self):
        status, body = self.call("/classes", self.model(adapter="no_such_adapter"))
        self.assertEqual(status, 400)
        self.assertIn("no adapter named 'no_such_adapter'", body["error"])
        self.assertEqual(self.call("/classes", self.model(adapter="/nowhere/adapter.py"))[0], 400)
        self.assertEqual(self.call("/predict", self.model(image="not base64!"))[0], 400)
        self.assertEqual(self.call("/predict", {"weights": "fake.pt"})[0], 400)
        status, body = self.call("/classes", self.model(weights="broken.pt"))
        self.assertEqual((status, body["error"]), (500, "RuntimeError: cannot read broken.pt"))
        status, body = self.call("/predict", self.model(image=base64.b64encode(b"explode").decode()))
        self.assertEqual((status, body["error"]), (500, "ValueError: the model exploded"))
        self.assertEqual(self.call("/nothing")[0], 404)


class ShapesTest(unittest.TestCase):
    def test_a_turned_box_comes_from_its_corners(self):
        # A 4 × 2 box turned a quarter clockwise on screen: its first side points down.
        box = shapes.oriented_from_corners([(0, 0), (0, 4), (-2, 4), (-2, 0)])
        self.assertEqual((box["centre"], box["width"], box["height"]), ({"x": -1.0, "y": 2.0}, 4.0, 2.0))
        self.assertAlmostEqual(box["angle"], math.pi / 2)

    def test_a_mask_is_its_bounding_box_run_length_encoded(self):
        try:
            import numpy as np
        except ImportError:
            self.skipTest("numpy is not installed")
        pixels = np.zeros((6, 8), np.uint8)
        pixels[2, 3:5] = 1
        pixels[3, 4] = 1
        self.assertEqual(shapes.mask(pixels), {"type": "mask", "left": 3, "top": 2, "width": 2, "height": 2, "rle": [0, 2, 1, 1]})
        self.assertIsNone(shapes.mask(np.zeros((2, 2))))


if __name__ == "__main__":
    unittest.main()
