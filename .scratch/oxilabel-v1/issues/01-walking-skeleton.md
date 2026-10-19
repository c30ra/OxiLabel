# 01: Walking skeleton

**What to build:** A single Podman image runs the Rust server with a SQLite database on a mounted volume and serves a Leptos web shell. In the browser I can create, list and delete a Project. This sets up the workspace crates (core, server, ui, formats) and the test harness at the server HTTP API seam.

**Blocked by:** None (can start immediately)

**Status:** done

- [x] Building the container image and running it with Podman serves the web UI on a port, with data stored on a mounted volume
- [x] I can create a Project with a name, see it in a Projects list, and delete it after confirmation
- [x] The Cargo workspace has the four crates, and the core crate compiles for both native and WASM
- [x] API tests run against an in-memory SQLite database and cover project create, list and delete

**Implementation note:** the UI is a client-side Leptos app built with `scripts/build-ui.sh` (wasm-bindgen-cli), not trunk, because trunk fails to compile with GCC 16 on the dev machine. The create/list/delete UI was verified manually in a browser.
