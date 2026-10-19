# OxiLabel

A self-hosted image annotation tool for machine-learning datasets: a Rust server used from the
browser, with an optional Python worker that pre-annotates images with your own models on your own
GPU.

- Rectangles, oriented rectangles, polygons, polylines, points, ellipses, brush masks and tags,
  drawn from pie menus with keyboard shortcuts.
- Large images load as tile pyramids. Enhancement filters (illumination flattening, CLAHE) change
  only the view, so annotations stay in original-image coordinates.
- Import CVAT "for images 1.1" exports. Export YOLO detect, OBB and segment datasets, or whole
  projects as lossless project archives.
- Accounts for admins and annotators, jobs and statistics.
- Pre-annotation with Ultralytics or any model you wrap in a small Python adapter.
- A command-line client, `oxilabel`, for imports, exports and pre-annotation.

## Install with the prebuilt container

Images are published to the GitHub Container Registry: `latest` follows `main`, and each release tag
`vX.Y.Z` gets `X.Y.Z` and `X.Y`.

    podman run -d --name oxilabel -p 3000:3000 \
        -v oxilabel-data:/data \
        -v /path/to/your/datasets:/images:ro \
        ghcr.io/c30ra/oxilabel:latest

Docker works the same way: replace `podman` with `docker`.

Open http://localhost:3000. While the server has no accounts, it asks you to create the first one,
which becomes the admin.

- `/data` holds the database (`oxilabel.db`), tile and thumbnail caches, uploads and exports. Keep
  it on a volume.
- `/images` is the folder that image imports browse. Mount your datasets there, read-only is
  enough.

To update, pull the new image and recreate the container with the same volumes:

    podman pull ghcr.io/c30ra/oxilabel:latest
    podman rm -f oxilabel
    # then the same podman run command as above

## Build from source

You need Rust (stable), the `wasm32-unknown-unknown` target and `wasm-bindgen-cli` at the same
version as the `wasm-bindgen` crate in `Cargo.lock`.

    rustup target add wasm32-unknown-unknown
    cargo install wasm-bindgen-cli --locked --version <version from Cargo.lock>
    scripts/build-ui.sh release
    cargo build --release -p oxilabel-server
    target/release/oxilabel-server

Run it from the repository root: by default it serves the UI from `crates/oxilabel-ui/dist`, keeps
its data in `./data` and imports from `./images`.

To build the container image yourself instead:

    podman build -f Containerfile -t oxilabel .

## Configuration

The server reads these environment variables. The container sets the paths to `/data` and
`/images`.

| Variable | Default | Meaning |
| --- | --- | --- |
| `OXILABEL_ADDR` | `0.0.0.0:3000` | Address and port to listen on |
| `OXILABEL_DATABASE` | `sqlite://data/oxilabel.db?mode=rwc` | SQLite database URL |
| `OXILABEL_IMAGES_ROOT` | `images` | Folder that image imports browse |
| `OXILABEL_CACHE_DIR` | `data/cache` | Tile and thumbnail cache |
| `OXILABEL_UPLOAD_DIR` | `data/uploads` | Images uploaded from the browser |
| `OXILABEL_EXPORT_DIR` | `data/exports` | Finished exports |
| `OXILABEL_WEB_ROOT` | `crates/oxilabel-ui/dist` | Built web UI |
| `OXILABEL_FOLDER_DIALOG` | on | `off` disables the desktop folder picker (it's never available in a container) |

The session cookie has no `Secure` flag, so the server can be used over plain HTTP on a local
network. To reach it over the internet, put it behind a reverse proxy that terminates HTTPS.

## Pre-annotation

The server sends images to a Python worker (`worker/`) that runs models through adapters. An admin
sets where the worker is on the Settings screen. See [docs/pre-annotation.md](docs/pre-annotation.md)
for running the worker next to the server, on the host of a containerized server, or in its own GPU
container. Read that page's Trust section before giving anyone an admin account.

## Command-line client

    cargo install --path crates/oxilabel-cli
    oxilabel login --server http://localhost:3000 --user admin
    oxilabel projects
    oxilabel export --project 1 --format yolo-detect --out dataset.zip

`oxilabel --help` lists every command: login, logout, projects, models, import (CVAT export or
project archive), export and pre-annotate.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
