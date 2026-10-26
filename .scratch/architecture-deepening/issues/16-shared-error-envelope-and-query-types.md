# 16: Shared error envelope and query types

**What to build:** The server, the web UI and the CLI share one error envelope type (also used for the worker's errors and streamed progress events), the host folder dialog replies, login credentials, and the query parameters of every endpoint that takes them (image page, neighbours, image ids, tiles, label delete confirmation, archive export). Each client reads errors in one place, with one fallback message and one signed-out handling. Renaming a query key or an error field then fails to compile instead of failing at run time.

**Blocked by:** None (can start immediately)

**Status:** done

- [x] The error envelope is one type in core, produced by the server and read once by the UI and once by the CLI
- [x] The host dialog replies and login credentials are no longer defined twice or built as raw JSON
- [x] The UI and CLI build every query string from the shared query types, as image filters already do
- [x] UI, CLI and server tests pass; one test round-trips each shared query type through the server's parser

## Comments

The shared wire types live in `oxilabel_core/src/api.rs`: `ErrorReply` (the worker's and every streamed `error` event's body too), `HostReply`, `FolderDialogReply`, and the query types `BrowseQuery`, `ImagePageQuery`, `NeighboursQuery`, `TileQuery`, `DeleteLabelQuery`, `ArchiveExportQuery`. Each query type has `to_query()`; `with_query` percent-encodes the pairs for links and CLI downloads, while the UI's `get_with_query` takes them directly.

The server handlers now extract the same structs instead of private duplicates; `ImagePageQuery` sits beside `ImageFilter` in the same handler. `oxilabel-server/tests/query_types.rs` parses every shared query type's `to_query()` with axum's `Query`, including defaults and special characters. A browser flow now pages the image grid (`the image grid pages through its images`), next to the existing filtered-grid flows.

Clients read failures in one place: `ui/api.rs::failure` (with the single fallback and signed-out callback) and `cli/lib.rs::error_message` (with the session-ended suffix around `Client::send`). The CLI login sends `core::user::Credentials` rather than raw JSON. `core::worker::ErrorReply` was removed, with its test moved to `api`, so downstream users should import `oxilabel_core::api::ErrorReply`.
