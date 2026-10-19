"""A worker adapter for the browser tests: on any image, a confident crack and a doubtful finger
(boxes in the 1600 × 1000 test image). The option "delay" (seconds) makes it slow."""
import time

from oxilabel_worker import shapes


def load(weights, options):
    return weights


def classes(model):
    return ["crack", "finger"]


def predict(model, image, options):
    time.sleep(options.get("delay", 0))
    return [
        {"class": "crack", "confidence": 0.9, "shape": shapes.rectangle(100, 100, 300, 200)},
        {"class": "finger", "confidence": 0.3, "shape": shapes.rectangle(400, 400, 500, 450)},
    ]
