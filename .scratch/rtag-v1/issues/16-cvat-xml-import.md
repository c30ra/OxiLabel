# 16: CVAT XML 1.1 import

**What to build:** I can import a CVAT "for images 1.1" XML export into a Project, mapping CVAT labels to rtag Labels and its boxes, polygons, polylines and points to rtag Annotations. A report lists unmapped labels and unsupported shapes before the import is committed.

**Blocked by:** 08, 10

**Status:** ready-for-agent

- [ ] Importing an XML creates annotations on matching images, matched by file name
- [ ] CVAT labels are mapped to existing labels or created on request, and attributes are carried over
- [ ] A preview report lists unmapped labels, unmatched images and unsupported shapes, and nothing is written until I confirm
- [ ] Formats tests use sample CVAT exports, including one from the author's `defect_detection` project, and check the resulting model
