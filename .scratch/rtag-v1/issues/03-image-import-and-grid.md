# 03: Image import and grid

**What to build:** I can add Images to a Project by pointing at a folder mounted into the container. The server records each image's dimensions and content hash without copying files, and the project dashboard shows an image grid with thumbnails.

**Blocked by:** 01

**Status:** ready-for-agent

- [x] Importing a folder adds all supported images recursively and reports how many were added
- [x] Importing the same folder again skips images already known by hash or path
- [ ] The grid shows thumbnails ~~, annotation count~~ and status for each image (annotation count moved to ticket 05), with simple pagination or virtual scrolling
- [x] Unreadable or unsupported files are reported, not silently dropped
- [x] API tests cover import, re-import, and dimension and hash recording

**Implementation note:** imports are confined to `RTAG_IMAGES_ROOT` (mounted at `/images` in the container). After review the folder-name field was replaced by an import dialog (see the approved design canvas): it browses the image root (`GET /api/browse`), marks images already in the project, and imports a selection of folders and single images (`POST /api/projects/{id}/imports` with `paths`). Known paths are skipped without reading; new files are hashed with blake3 and content duplicates are skipped. Thumbnails (max 256 px, never upscaled) are generated on first request and cached by hash under `RTAG_CACHE_DIR`. Measured on the author's data: 487 files / 2.2 GB imported in 16 s, re-import 0.5 s, cold 6 MP thumbnail about 0.2 s.

**Known gap:** the browser marks images by path only, so a content duplicate under a new name shows as "new" and is then reported as skipped on import.

**Follow-up (not in this ticket):** the import runs inside one HTTP request. For folders of many thousands of images it should become a background job with progress; hashing could also be parallelised.
