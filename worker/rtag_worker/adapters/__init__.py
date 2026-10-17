"""Adapters: how the worker runs one kind of model. An adapter is a module, bundled here or a .py
file anywhere (the model's `adapter` is then its path), with three functions:

    load(weights: str, options: dict) -> model
        Loads the model from the weights file; `options` are the model's options in rtag.
    classes(model) -> list[str]
        The model's class names, which rtag maps to labels.
    predict(model, image: bytes, options: dict) -> list[dict]
        What the model finds on the image (the bytes of its file; `rtag_worker.shapes.decode`
        turns them into an OpenCV image): dicts with "class" (a name from `classes`),
        "confidence" (0 to 1) and "shape" (built with `rtag_worker.shapes`, in the image's own
        pixels, whatever the model ran on).

A sketch of an adapter for a model with its own code:

    import sys; sys.path.insert(0, "/path/to/the/model/code")
    from rtag_worker import shapes
    from mymodel import Detector

    def load(weights, options):
        return Detector(weights, **options)

    def classes(model):
        return model.class_names

    def predict(model, image, options):
        found = model.run(shapes.decode(image))
        return [{"class": f.name, "confidence": f.score, "shape": shapes.polygon(f.outline)} for f in found]
"""
