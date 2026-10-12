# 10: Tags and attributes

**What to build:** I can apply an image-level Tag with a Label to a whole Image, and define Attributes (checkbox, select, number, text, with defaults) on a Label. An attribute editor appears only when the selected annotation or tag has attributes.

**Blocked by:** 05

**Status:** ready-for-agent

- [ ] The pie menu offers a tag tool, and tags are listed for the image
- [ ] Labels can define attributes with types and defaults in the label screen
- [ ] Selecting an annotation or tag whose label has attributes shows an editor, otherwise none is shown
- [x] Attribute values persist with the annotation and reload correctly
- [x] API tests cover attribute validation against the label definition

**Implementation note (2026-10-12):** decisions asked of the user: a tag is applied from the pies (Tag in the tool pie, then a label; or T then a label key), leaving the current tool unchanged, and picking a label the image already has as a tag removes it; tags are shown both as chips over the image (click selects, × removes) and in a "Tags" section of a side panel right of the image. Core: `rtag_core::attribute` (`AttributeDefinition` with `AttributeKind` checkbox / select with options / number with optional min and max / text, each with a default; `AttributeValue`; `Label::check_attributes` and `conform_attributes`); labels carry `attributes`, annotations an `attributes` map (left out of the JSON when empty), and a tag is `Shape::Tag`, an annotation without geometry that drawing, hit-testing, box selection and YOLO export skip. Server: migration 0006 adds the columns; saves are checked against the label (unknown names, wrong types, select values not in the options, numbers out of range, and a tag repeated on one image are rejected); editing a label's attributes conforms the stored values of its annotations (removed attributes dropped, values that no longer fit and missing ones set to defaults) and bumps those images' versions. UI: the label form lists attributes (name, type, options or range, default; "Add attribute"); new annotations, strokes, fills and tags get their label's defaults, relabelling conforms the values, and loaded annotations are conformed to the current labels; the side panel shows the image's tags and, when one annotation or tag is selected whose label has attributes, an editor with a field per attribute (each change one undo step). The manifest's annotation count includes tags.
