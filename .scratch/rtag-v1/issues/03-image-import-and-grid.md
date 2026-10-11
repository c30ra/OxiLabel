# 03: Image import and grid

**What to build:** I can add Images to a Project by pointing at a folder mounted into the container. The server records each image's dimensions and content hash without copying files, and the project dashboard shows an image grid with thumbnails.

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] Importing a folder adds all supported images recursively and reports how many were added
- [ ] Importing the same folder again skips images already known by hash or path
- [ ] The grid shows thumbnails, annotation count and status for each image, with simple pagination or virtual scrolling
- [ ] Unreadable or unsupported files are reported, not silently dropped
- [ ] API tests cover import, re-import, and dimension and hash recording
