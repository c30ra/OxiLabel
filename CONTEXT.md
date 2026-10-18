# rtag

A self-hosted tool for labeling image datasets (initially for ML object detection and segmentation), used alone or by a team, and exporting them in formats that major ML frameworks accept (e.g. YOLO). Vocabulary follows CVAT/COCO conventions.

## Language

**Project**:
A named collection of images together with the label set used to annotate them.
_Avoid_: Dataset, task

**Job**:
A batch of images in a project assigned to one annotator and worked as a unit.
_Avoid_: Task, assignment

**Image**:
A single picture inside a project that can carry annotations.
_Avoid_: Sample, frame, photo

**Label**:
A category defined at project level (e.g. "crack") that annotations and tags refer to. It has a permanent numeric id, a color, a hotkey, the shape types it allows, and its attributes.
_Avoid_: Class, category

**Ignore label**:
A label marking regions whose content is uncertain, which training must skip; mask export writes it as the ignore value.
_Avoid_: Unknown, void

**Annotation**:
One shape drawn on an image (rectangle, polygon, polyline, points, mask, ...) that refers to a label.
_Avoid_: Tag, box, region

**Tag**:
An image-level classification: a label applied to the whole image rather than to a shape.
_Avoid_: Image label, category

**Attribute**:
Extra named property attached to an annotation or tag (e.g. "occluded"), beyond its label.
_Avoid_: Metadata, property

**Shape constraint**:
A label's fixed size for one shape type (width and height of a rectangle or oriented rectangle, the two radii of an ellipse), applied when an annotator draws that shape for that label. It guides drawing; it does not invalidate annotations that differ from it.
_Avoid_: Fixed size, template, preset

**Export footprint**:
The area an exporter turns a polyline or points into: the line stroked at its label's line width, each point as a disc or square of its label's point radius. The label's values are the defaults; a single export may override them per label. The editor can show it as a ghost so the annotator sees what will be exported.
_Avoid_: Ghost (for the area itself), buffer, halo

**Enhancement filter**:
A view-only transformation of an image (e.g. illumination flattening, local contrast) that makes faint structures visible. It never changes the image or where annotations lie; image-analysis aids such as edge snapping and fuzzy fill work on the original image unless the annotator chooses to use the filtered view.
_Avoid_: Preprocessing, effect, enhancement (on its own)

**Exporter**:
A pluggable component that converts a project's annotations into one external format (e.g. YOLO detect or segment) for a given model type.
_Avoid_: Converter, writer

**Project archive**:
rtag's own versioned, lossless file format for moving a whole project (labels, images or image references, annotations) between rtag instances.
_Avoid_: Backup, dump

**Pre-annotation**:
An annotation produced by a model rather than a person, carrying its source and confidence, to be reviewed before it counts.
_Avoid_: Pseudo-label, auto-label, prediction
