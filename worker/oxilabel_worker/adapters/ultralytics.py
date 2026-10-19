"""Ultralytics models (YOLO detect, OBB, segment, classify), by the task their weights were
trained for: detect boxes become rectangles, OBB boxes oriented rectangles, segment masks
polygons (or masks with the option "masks": true), classify results an image tag.

Options: "conf" (default 0.25), "iou", "imgsz", "device", "max_det" are passed to Ultralytics'
predict; "masks": true returns segment results as brush masks instead of polygons.
"""
from oxilabel_worker import shapes

PREDICT_OPTIONS = ("conf", "iou", "imgsz", "device", "max_det", "half", "augment")


def load(weights, options):
    from ultralytics import YOLO

    return YOLO(weights)


def classes(model):
    return [model.names[i] for i in sorted(model.names)]


def predict(model, image, options):
    picture = shapes.decode(image)
    if picture.ndim == 2:
        import cv2

        picture = cv2.cvtColor(picture, cv2.COLOR_GRAY2BGR)
    settings = {key: options[key] for key in PREDICT_OPTIONS if key in options}
    result = model.predict(picture, verbose=False, **settings)[0]
    names = model.names
    found = []

    def add(class_id, confidence, shape):
        if shape is not None:
            found.append({"class": names[int(class_id)], "confidence": float(confidence), "shape": shape})

    if model.task == "classify":
        add(result.probs.top1, result.probs.top1conf, shapes.tag())
    elif model.task == "obb":
        for corners, class_id, confidence in zip(result.obb.xyxyxyxy.tolist(), result.obb.cls.tolist(), result.obb.conf.tolist()):
            add(class_id, confidence, shapes.oriented_from_corners(corners))
    elif model.task == "segment" and result.masks is not None:
        boxes = result.boxes
        if options.get("masks"):
            import cv2

            height, width = picture.shape[:2]
            for data, class_id, confidence in zip(result.masks.data.cpu().numpy(), boxes.cls.tolist(), boxes.conf.tolist()):
                # Masks come at the model's resolution; bring them to the image's.
                full = cv2.resize(data, (width, height), interpolation=cv2.INTER_LINEAR) > 0.5
                add(class_id, confidence, shapes.mask(full))
        else:
            for outline, class_id, confidence in zip(result.masks.xy, boxes.cls.tolist(), boxes.conf.tolist()):
                if len(outline) >= 3:
                    add(class_id, confidence, shapes.polygon(outline.tolist()))
    else:
        for box, class_id, confidence in zip(result.boxes.xyxy.tolist(), result.boxes.cls.tolist(), result.boxes.conf.tolist()):
            add(class_id, confidence, shapes.rectangle(*box))
    return found
