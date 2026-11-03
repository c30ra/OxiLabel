# Images come from any folder of the host, picked in its own dialog, or are uploaded

When the browser runs on the same machine as the server, choosing a folder of images opens the desktop's own folder dialog, opened by the server (through the XDG desktop portal), and the images are referenced at their absolute paths wherever they are; the image root (`OXILABEL_IMAGES_ROOT`) is no longer a limit. When the browser is on another machine, or the server cannot show a dialog (e.g. headless in Podman), the folder is picked in the browser's dialog and its images are uploaded: the server keeps copies under its data folder and references those. This replaces the v1 rule that images are only referenced from a mounted folder and never copied.

The browser alone cannot do this: its folder dialog shows the files of the machine the browser runs on and never reveals a folder's path, so the server could not reference them.

"Same machine" means the request comes over loopback. Imports by absolute path, and opening the host dialog, are refused for any other request: otherwise anyone who can reach the server could have it read arbitrary files and serve them back as thumbnails and tiles.

## Considered options

- Browser dialog, then find the folder by name under the image root: guesswork when names repeat, and still limited to the image root.
- Keep the server-side folder browser of the image root: works everywhere, but it is not the system dialog and cannot reach folders outside the root.

## Consequences

- Image paths are either relative to the image root (CVAT and archive imports) or absolute (host dialog, uploads).
- The same goes for single images: on the server's machine the desktop's file dialog picks one or more, referenced where they are; otherwise the browser's file dialog picks them and they are uploaded. The server-side folder browser of the image root is gone.
- A dataset picked on another machine is copied once to the server; deduplication by content hash keeps re-uploads from doubling it.
