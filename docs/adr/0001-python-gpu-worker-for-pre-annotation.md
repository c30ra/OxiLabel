# Model inference runs in a Python worker, not in the Rust server

Pre-annotation runs in a separate Python worker process (Ultralytics/PyTorch on the local ROCm GPU) that the Rust server calls over local HTTP/JSON and that ships in the same Podman image. This lets rtag reuse existing Ultralytics models (detect, OBB, segment) and the user's GPU unchanged, which is the main thing CVAT's separate-container model blocked. We rejected ONNX Runtime inside Rust because ROCm/MIGraphX support is the least reliable part of that stack; the narrow worker interface (images in, pre-annotations out) keeps that option open later.

## Update (2026-10-17, ticket 17)

The worker no longer has to ship in the server's image. The server starts it itself when given `RTAG_WORKER_COMMAND` (e.g. a ROCm venv's Python on the host), or talks to one started elsewhere at `RTAG_WORKER_URL`: a containerised server cannot start a process on the host, and the user's ROCm PyTorch already lives in a host venv. Images go to the worker as bytes, so server and worker need not see the same files. The worker only implements an adapter interface (`worker/rtag_worker/adapters`), so Ultralytics models and the user's own networks run alike.
