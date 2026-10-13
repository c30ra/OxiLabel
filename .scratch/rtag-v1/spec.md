Status: ready-for-agent

# rtag v1: self-hosted image annotation tool

## Problem Statement

I label image datasets for ML (detection, oriented boxes, segmentation) and currently use CVAT. It is slow and clunky, its interface is hard to use, and its auto-labeling needs a separate container, which prevents me from running inference on my own GPU (AMD ROCm, Ultralytics models). It also lacks the image-enhancement tools I need to see faint structures (illumination flattening, local contrast), which I currently bake into images with ad-hoc scripts and then map back to the originals by hand. I need a faster, more usable tool that I can run myself now and share with a small team later.

## Solution

rtag is a self-hosted web application: a Rust server (with a Python GPU worker alongside it) in one Podman image, used from the browser. I create a **Project** with a **Label** set, add **Images** (referenced from a mounted folder), split them into **Jobs**, and annotate them in a fast canvas workspace driven by a two-layer pie menu and image-editor shortcuts. Large images load as lazily generated tile pyramids, and enhancement filters are view-only so annotations always stay in original-image coordinates. A model can run on my GPU to produce **Pre-annotations** that I review. I import existing CVAT projects, and export through pluggable **Exporters** (YOLO detect/OBB/segment, mask PNG), or move whole projects between rtag instances as a **Project archive**. A small CLI covers import/export and headless pre-annotation.

## User Stories

Projects and labels

1. As an admin, I want to create a project with a name, so that I can group related images and labels.
2. As an admin, I want to see a list of all projects with progress at a glance, so that I can pick up where I left off.
3. As an admin, I want to define labels with a name, color and hotkey, so that I can recognize and select them quickly while drawing.
4. As an admin, I want each label to have a permanent numeric id that is never reused or renumbered, so that exports and trained models stay consistent as the label set evolves.
5. As an admin, I want to restrict a label to certain shape types, so that the editor prevents drawing a shape that makes no sense for that label.
6. As an admin, I want to define attributes on a label (checkbox, select, number, text) with defaults, so that I can record extra properties like "occluded".
7. As an admin, I want to mark a label as an ignore label, so that uncertain regions are exported with the ignore value and skipped in training.
8. As an admin, I want to rename or recolor a label without breaking existing annotations, so that I can fix mistakes cheaply.
9. As an admin, I want to be prevented from deleting a label that is in use (or be told what will be removed), so that I do not destroy annotations by accident.
10. As an admin, I want to delete a project, with confirmation, so that I can clean up finished work.

Images and jobs

11. As an admin, I want to add images to a project by pointing at a mounted folder, so that large datasets are not copied.
12. As an admin, I want the server to read image dimensions and record a content hash on import, so that images can be tracked reliably.
13. As an admin, I want re-importing the same folder to skip already known images, so that I can add new files incrementally.
14. As an admin, I want to see an image grid with thumbnails, annotation counts and status, so that I can survey a project.
15. As an admin, I want to filter the grid by status, label and job, so that I can find what needs work.
16. As an admin, I want to split a project's images into jobs (batches), so that work can be assigned.
17. As an admin, I want to assign a job to an annotator, so that each person has a clear workload.
18. As an annotator, I want to see the jobs assigned to me, so that I know what to work on.
19. As an annotator, I want each image to show as annotated or not annotated, and to set that myself, so that progress is tracked.
20. As an annotator, I want to move to the next or previous image with keys, so that I can work through a job quickly.
21. As an annotator, I want a filmstrip of neighbouring images with their status, so that I can see context and jump around.
22. As an admin, I want a statistics view of annotation counts per label, per job and per user, so that I can see progress and balance.

Viewing

23. As an annotator, I want large images (6 MP and up) to open quickly through a tile pyramid, so that the editor stays responsive.
24. As an annotator, I want tiles generated lazily on first view and cached, so that importing a big folder is instant.
25. As an annotator, I want to zoom with the scroll wheel and pan with Space-drag, so that navigation feels like an image editor.
26. As an annotator, I want the full-resolution detail to load as I zoom in, so that I can place points accurately.
27. As an annotator, I want a filter panel with presets (original, illumination flatten, CLAHE, enhance_v2) and live parameter sliders, so that faint structures become visible.
28. As an annotator, I want a hold-key that temporarily shows the original image, so that I can compare against the enhanced view.
29. As an annotator, I want my filter settings remembered per project, so that I do not retune them every session.
30. As an annotator, I want annotations to remain in original image coordinates regardless of the active filter, so that exports are always correct.
31. As an annotator, I want filtered views to be cached on the server, so that switching filters is fast the second time.

Drawing and editing

32. As an annotator, I want to hold Q (or the right mouse button) to open a pie menu at my cursor, so that I can choose without moving my hand to a toolbar.
33. As an annotator, I want the first ring to show tools (rectangle, oriented rectangle, polygon, polyline, points, brush, tag) and the second ring to show labels, so that one flick pair picks tool and label.
34. As an annotator, I want the label ring to show about 8 slices with pinned and recent labels first and a way to reach the rest by a "more" slice or type-to-search, so that projects with dozens of labels stay usable.
35. As an annotator, I want number hotkeys to select labels directly, so that switching is instant.
36. As an annotator, I want a quick tap of the pie key to repeat my last tool and label, so that repetitive labeling is fast.
37. As an annotator, I want pressing the pie key while a tool is active to open that tool's options (e.g. 2-point or 3-point rectangle, brush size, snap angle), so that options are close at hand.
38. As an annotator, I want Esc to first discard a shape in progress and then, if nothing is in progress, return to tool-choose mode, so that I can back out gradually.
39. As an annotator, I want to switch label while staying in a tool, either with number keys plus a label-ring key or via a label slice in the options ring, and to choose which style I use, so that it fits how I work.
40. As an annotator, I want to draw rectangles with two points, so that axis-aligned boxes are quick.
41. As an annotator, I want to draw oriented rectangles with three points, so that rotated boxes are quick.
42. As an annotator, I want to draw polygons by clicking vertices and finishing with Enter or double-click, so that I can outline objects.
43. As an annotator, I want to draw polylines (for example hand-drawn centre lines of cracks), so that thin structures can be annotated.
44. As an annotator, I want to place points, so that I can mark small locations.
45. As an annotator, I want a brush and eraser with resizable size (`[` and `]`), so that I can paint masks.
46. As an annotator, I want Shift to constrain lines to fixed angles or rectangles to squares, and Alt to draw from the center, so that precise shapes need no extra tools.
47. As an annotator, I want to apply an image-level tag with a label, so that I can classify whole images.
48. As an annotator, I want to select a shape by clicking it, drag its handles and vertices, add or remove vertices, and press Delete to remove it, so that corrections are direct.
49. As an annotator, I want to change the label of a selected shape from the pie menu, so that relabeling is quick.
50. As an annotator, I want an attribute editor that appears only when the selected annotation's label has attributes, so that the screen stays uncluttered.
51. As an annotator, I want a collapsible objects list where I can select, hide, lock and relabel annotations, so that I can manage busy images.
52. As an annotator, I want Ctrl+Z and Ctrl+Y to undo and redo within the image, so that mistakes are cheap.
53. As an annotator, I want my changes autosaved per image when I change or leave it, so that I never lose work.
54. As an annotator, I want a clear indication of unsaved or failed saves, so that I can trust the data.
55. As an annotator, I want a save to be rejected with a clear message when someone else changed the same image meanwhile, so that no annotations are silently overwritten.
56. As an annotator, I want my job to be owned by me, so that others do not edit the same images.

Pre-annotation

57. As an annotator, I want to run a registered model on an image, a selection or a whole job using my local GPU, so that I do not draw everything by hand.
58. As an admin, I want to register models (weights file, type, class-to-label mapping) and see worker health, so that I know what is available.
59. As an annotator, I want pre-annotations marked with their source and confidence, so that I can tell them apart from manual work.
60. As an annotator, I want to filter pre-annotations by a confidence threshold, so that I can reduce noise before reviewing.
61. As an annotator, I want to accept, edit or delete pre-annotations individually or in bulk, so that I decide what counts as ground truth.
62. As an annotator, I want pre-annotations to be placed in original image coordinates even if I used an enhancement view, so that they line up with the image.
63. As an admin, I want a long inference run to report progress and be cancellable, so that it never blocks the app.

Import and export

64. As an admin, I want to import a CVAT "for images 1.1" XML export into a project, mapping its labels and shapes, so that my existing work is not lost.
65. As an admin, I want the import to report unmapped labels and unsupported shapes, so that I can fix them before committing.
66. As an admin, I want to export YOLO detect, YOLO OBB and YOLO segment, so that I can train Ultralytics models.
67. As an admin, I want to export label-driven mask PNGs with the ignore value, so that I can train my custom segmentation network.
68. As an admin, I want each export to write a small manifest (timestamp, label set, image count, annotation hash), so that I know exactly what a model was trained on.
69. As an admin, I want to choose which images (all, annotated only, a job) an export contains, so that I control the dataset.
70. As an admin, I want a pluggable exporter interface, so that new model formats can be added without touching the core.
71. As an admin, I want to export a whole project as a Project archive and import it into another rtag instance without loss, so that I can move or back up work.

Accounts and administration

72. As the first user, I want my account to become admin on first run, so that setup needs no manual database work.
73. As an admin, I want to create annotator accounts and reset passwords, so that I can bring in a team.
74. As a user, I want to log in with a username and password and stay signed in with a session cookie, so that I can use the tool securely.
75. As an annotator, I want to see and edit only jobs I own, while admins see everything, so that responsibilities are clear.
76. As a user, I want a settings screen for remappable keys, pie menu sensitivity, label-switch style and theme, so that the tool fits me.
77. As a user, I want my settings stored on the server, so that they follow me across machines.

Deployment and CLI

78. As an admin, I want to run the whole thing as one Podman image with the Python worker and GPU access, so that deployment is easy.
79. As an admin, I want a SQLite file and cache folders on mounted volumes, so that data survives container updates.
80. As an admin, I want a CLI to import projects, export formats and archives, and run pre-annotation headlessly, so that I can script routine work.

## Implementation Decisions

Architecture

- A Cargo workspace with four crates: a core crate (domain types, shapes, geometry, RLE, coordinate transforms, command history; compiles to native and WASM and is shared by both sides), a server crate (HTTP API, auth, SQLite, tile and filter generation, worker client), a UI crate (Leptos app shell plus a Rust/WASM canvas), and a formats crate (importers and exporters).
- The server is the single source of truth and serves the web UI. The browser is the only client in v1; no Tauri.
- Python GPU worker runs next to the server in the same image and is called over local HTTP/JSON; recorded in ADR 0001. Worker contract: list models, run a model on image ids with parameters, report progress, return Pre-annotations (shape, label mapping, confidence, source) in original image coordinates.
- SQLite is the database, with queries kept portable. Mask annotations are stored as RLE in original image coordinates.
- Images are referenced where they are: under the mounted image root, or (when the browser runs on the server's machine) any host folder picked in the desktop's own dialog; from a browser on another machine they are uploaded and the server keeps copies (ADR 0003). The server records dimensions and content hash on import.
- Auth: local accounts, argon2-hashed passwords, session cookie. Roles: admin, annotator. First created user is admin.
- Per-user settings (keymap, pie sensitivity, label-switch style, theme) are stored on the server.

Domain model (vocabulary from `CONTEXT.md`)

- Project (name, label set) contains Images and Jobs. A Job is a batch of images assigned to one annotator.
- Image status is annotated or not annotated. There is no review state and no annotation history in v1.
- Label: permanent numeric id (never reused), name, color, hotkey, allowed shape types, attributes, ignore flag.
- Annotation shapes in v1: rectangle, oriented rectangle, polygon, polyline, points, brush/mask. Tags are image-level label applications. All shapes carry attributes, a source (manual or model name) and optional confidence.
- Concurrency: job ownership plus a per-image version number checked on save; stale saves are rejected. No real-time co-editing.

Image serving and filters

- Tile pyramids are generated lazily on first view and cached on disk. Each enhancement filter and parameter set gets its own cached pyramid.
- Filters (illumination flatten, CLAHE, enhance_v2 and basic brightness/contrast) are view-only, computed server-side. Reference implementations exist in the author's `defect_detection` project (`seg_prep/common.py`) and should be reproduced faithfully, including parameters.

Editor interaction

- Two-layer pie menu opened by holding Q or the right mouse button at the cursor; flick to select. Ring 1 tools, ring 2 labels (about 8 slices, pinned/recent first, "more" slice or type-to-search). A quick tap repeats last tool+label.
- In tool-choose mode the key opens the tool then label rings; in editing mode it opens the active tool's options ring. Esc is two-step (discard in-progress shape, then return to tool-choose mode).
- Label switching in editing mode supports two user-selectable styles: number hotkeys plus a label-ring key, or a label slice in the options ring.
- Image-editor shortcut conventions (Space-drag pan, scroll zoom, `[` `]` brush size, Shift constrains angle/square, Alt draws from center, Ctrl+Z/Y, B/E/V tool keys). Select is the default tool.
- Persistent UI is limited to a collapsible objects list, a filmstrip, and an attribute editor shown only when relevant. Undo/redo is a client-side command history; annotations autosave per image as a batch.

Screens

- Projects list and dashboard (image grid, jobs, labels, import/export), annotation workspace, admin (users, models and worker health), statistics, settings.

Import/export

- Importer: CVAT "for images 1.1" XML (with an unmapped-label and unsupported-shape report).
- Exporters (pluggable interface): YOLO detect, YOLO OBB, YOLO segment, mask PNG (label ids, ignore value from the ignore label). Each export writes a manifest.
- Project archive: versioned zip with `project.json` (labels, attributes, jobs, annotations in original coordinates, image references with hashes), optionally including images. It is the only format guaranteed to round-trip.

CLI

- Subcommands for import, export (formats and archive) and pre-annotation. It uses the same operations as the server API; user administration is out of scope.

## Testing Decisions

- A good test exercises external behavior through a public interface and asserts on outputs and persisted results, never on internal structure.
- Seams (all four confirmed):
  - Server HTTP API: tests run against an in-memory SQLite and a fake worker, covering projects, labels, jobs, annotation save with versioning, auth and roles, pre-annotation flow, tiles and import/export end to end.
  - Formats API: fixture-based tests of CVAT XML to model, and model to YOLO/mask PNG/archive and back, including archive round-trips.
  - Core geometry API: unit tests of shape math, RLE encode/decode, coordinate transforms and the command history.
  - UI canvas interaction: browser-level tests of the pie menu, drawing and editing flows. These are the heaviest, so keep the set small and focused on the main flows.
- Prior art: none in this repo. The author's `defect_detection` scripts (`seg_prep/cvat_to_masks.py`, `seg_prep/common.py`) can be used to generate expected outputs for mask and filter fixtures.

## Out of Scope

- Video, object tracking and interpolation, 3D and cuboids.
- Skeletons and ellipses (planned for a later version).
- Review/approval workflow, annotation history and audit log, reproducible dataset snapshots.
- Real-time co-editing.
- Tauri desktop shell and offline/standalone mode.
- Uploading images into server storage and object storage backends.
- Derived images (cropping and straightening with stored transforms).
- COCO JSON export, OIDC/SSO, Postgres, touch and pen-tablet input.
- Anomaly-score sample selection and training pipelines from the author's other project.
- CLI user and server administration.

## Further Notes

- The design came from a long grilling session; `CONTEXT.md` holds the glossary and `docs/adr/0001-python-gpu-worker-for-pre-annotation.md` the worker decision.
- The author's workflow in `defect_detection` (ROCm gfx1101 GPU, 6 MP electroluminescence images, polylines dilated to masks, an ignore label with value 255, label ids that are never renumbered) is the reference use case.
- Suggested first thin vertical slice: create a project, show one image through the tile pyramid, draw one rectangle, export it as YOLO.
