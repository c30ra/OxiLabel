"""An adapter for the worker's tests: no model, a rectangle as wide as the image file is long."""
from rtag_worker import shapes

loads = []


def load(weights, options):
    loads.append((weights, options))
    if weights == "broken.pt":
        raise RuntimeError("cannot read broken.pt")
    return {"weights": weights}


def classes(model):
    return ["crack", "chip"]


def predict(model, image, options):
    if image == b"explode":
        raise ValueError("the model exploded")
    return [{"class": "crack", "confidence": options.get("conf", 0.5), "shape": shapes.rectangle(0, 0, len(image), 1)}]
