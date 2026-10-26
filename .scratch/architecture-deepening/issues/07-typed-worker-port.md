# 07: Typed worker port with an in-process fake

**What to build:** The server talks to the pre-annotation worker through a typed port: the request (adapter, weights, options, image bytes) and the reply (classes, predictions, or the worker's error) are types in core instead of hand-built JSON. Two adapters sit at the port: the HTTP adapter for the Python worker (the wire format stays exactly as ADR 0001 and the worker define it) and one in-process fake shared by the server tests and the CLI tests, replacing the two separate fake HTTP workers. Starting and restarting the worker process from the Settings screen is a separate supervisor, apart from the port.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [x] Worker requests and replies are typed in core; no JSON is built or read by key for them in the server
- [x] The worker's default address is defined once on the Rust side
- [x] Server and CLI tests use one shared fake; the two hand-written fake workers are gone
- [x] A test feeds the Python worker's shape output for each shape type through the Rust shape parser
- [x] The Python worker tests and the CLI end-to-end test still pass against the real HTTP adapter

## Comments

**Implementation note:** the wire types are in `oxilabel-core/src/worker.rs`: `WorkerModel` (adapter, weights, options; the `/classes` body, `From<&ModelDefinition>`), `PredictRequest` (the model flattened plus the image bytes, base64 on the wire), the replies `HealthReply`/`ClassesReply`/`PredictReply`/`ErrorReply`, and `WorkerError` (`Unreachable`, `Failed` with the worker's reason, `Unreadable`), whose messages are the old ones. Its tests pin the JSON against the Python worker's contract. The port is `oxilabel_server::worker::WorkerPort` (`health`, `classes`, `predict`; async fns returning `Send` futures, generic, no boxing), with the HTTP adapter `HttpWorker` next to it; `worker::Worker` holds the adapter for the URL an admin set (none: no worker). The supervisor stays `worker_process::WorkerProcess`, now its own `AppState` field; `inference::apply_worker_settings` points the port at the URL and restarts the command. The run loop reuses one `PredictRequest` and swaps in each image's bytes. The default address is still only `inference::DEFAULT_WORKER_URL` (the settings test now uses it instead of a literal). The shared fake is `oxilabel_server::fake_worker::FakeWorker` behind the server's `fake-worker` feature (the server enables it on itself as a dev-dependency, the CLI as well). It answers in process through the port, or over HTTP as the Python worker does (`serve()`), so the server's and the CLI's HTTP tests go through the real `HttpWorker`. Its default device, classes and four predictions are the old server fake's, and it records each request (`requests()`). The CLI test uses the same defaults; its assertions hold unchanged. The shapes check is `worker/tests/shapes.json`, one case per helper and every shape type, mask included: the Python test checks the helpers' output against it (the mask case needs numpy, checked with `uv run --with numpy`), and `every_shape_the_python_worker_writes_is_read` in core parses it into the expected `Shape`s.
