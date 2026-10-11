# 06: YOLO detect export

**What to build:** The pluggable Exporter interface and its first implementation, YOLO detect. From a project I can export all images, only annotated ones, or a chosen set, and the export writes a manifest (timestamp, label set, image count, annotation hash). This completes the first end-to-end slice: project, image, rectangle, export.

**Blocked by:** 05

**Status:** ready-for-agent

- [x] An export produces YOLO detect label files and the class mapping using each label's permanent id
- [ ] I can choose which images the export contains
- [x] Every export writes a manifest next to the output
- [x] A new exporter can be added by implementing the interface, without changing core or server code
- [x] Formats tests compare exported files against fixtures, and an API test covers the export endpoint

**Implementation note:** `rtag-formats` has the `Exporter` trait (exporters write through an `ExportSink`; a new format = implement the trait and list it in `exporters()`), `run_export` (also writes `manifest.json`: format, UTC time, project, labels, counts, annotation hash, image mode, split and per-subset counts) and `YoloDetect`: `images/<subset>/<flat name>` and `labels/<subset>/<flat stem>.txt` (`class cx cy w h`, 6 decimals), class = label id − 1, `data.yaml` names every class up to the highest id (`unused_N` placeholders), ignore-label boxes left out, empty label files for images without boxes. Train/val/test split (default 80/20/0, chosen per export) assigns each image by its content hash, so subsets stay stable across exports; `data.yaml` points val at train when no image falls in val. Server: `GET /api/exporters`, `POST /api/projects/{id}/exports` (`images`: all / with_annotations / ids; `include_images`: none / symlink / copy) writes `RTAG_EXPORT_DIR/<project>-<format>-<UTC stamp>/` (container: `/data/exports`), `GET /api/exports/{name}/zip` builds the zip once (following links) and streams it. UI: "Export…" in the project header; "Selected images" uses the grid's selection. Symlinks point at the server's image paths (`/images/…` inside the container), so on the host they only resolve when the paths match; choose copies or the zip otherwise. "A job" as a selection comes with ticket 15. Exporting all 947 images of the author's project (labels only) takes 0.02 s.

