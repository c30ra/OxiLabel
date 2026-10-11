# 06: YOLO detect export

**What to build:** The pluggable Exporter interface and its first implementation, YOLO detect. From a project I can export all images, only annotated ones, or a chosen set, and the export writes a manifest (timestamp, label set, image count, annotation hash). This completes the first end-to-end slice: project, image, rectangle, export.

**Blocked by:** 05

**Status:** ready-for-agent

- [ ] An export produces YOLO detect label files and the class mapping using each label's permanent id
- [ ] I can choose which images the export contains
- [ ] Every export writes a manifest next to the output
- [ ] A new exporter can be added by implementing the interface, without changing core or server code
- [ ] Formats tests compare exported files against fixtures, and an API test covers the export endpoint
