# 07: Typed worker port with an in-process fake

**What to build:** The server talks to the pre-annotation worker through a typed port: the request (adapter, weights, options, image bytes) and the reply (classes, predictions, or the worker's error) are types in core instead of hand-built JSON. Two adapters sit at the port: the HTTP adapter for the Python worker (the wire format stays exactly as ADR 0001 and the worker define it) and one in-process fake shared by the server tests and the CLI tests, replacing the two separate fake HTTP workers. Starting and restarting the worker process from the Settings screen is a separate supervisor, apart from the port.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] Worker requests and replies are typed in core; no JSON is built or read by key for them in the server
- [ ] The worker's default address is defined once on the Rust side
- [ ] Server and CLI tests use one shared fake; the two hand-written fake workers are gone
- [ ] A test feeds the Python worker's shape output for each shape type through the Rust shape parser
- [ ] The Python worker tests and the CLI end-to-end test still pass against the real HTTP adapter
