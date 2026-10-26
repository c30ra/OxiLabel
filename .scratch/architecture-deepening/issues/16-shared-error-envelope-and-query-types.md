# 16: Shared error envelope and query types

**What to build:** The server, the web UI and the CLI share one error envelope type (also used for the worker's errors and streamed progress events), the host folder dialog replies, login credentials, and the query parameters of every endpoint that takes them (image page, neighbours, image ids, tiles, label delete confirmation, archive export). Each client reads errors in one place, with one fallback message and one signed-out handling. Renaming a query key or an error field then fails to compile instead of failing at run time.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] The error envelope is one type in core, produced by the server and read once by the UI and once by the CLI
- [ ] The host dialog replies and login credentials are no longer defined twice or built as raw JSON
- [ ] The UI and CLI build every query string from the shared query types, as image filters already do
- [ ] UI, CLI and server tests pass; one test round-trips each shared query type through the server's parser
