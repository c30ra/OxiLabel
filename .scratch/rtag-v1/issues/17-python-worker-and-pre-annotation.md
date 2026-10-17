# 17: Python worker and pre-annotation

**What to build:** A Python worker in the same image runs registered Ultralytics models on the local GPU and returns Pre-annotations over local HTTP/JSON (see ADR 0001). I can register a model, run it on an image, a selection or a Job with progress and cancel, then filter by confidence and accept, edit or delete the results.

**Blocked by:** 05

**Status:** ready-for-agent

- [ ] The admin screen shows worker health and lets me register a model with its type, weights and class-to-label mapping
- [ ] Running a model on images creates Pre-annotations marked with their source and confidence, in original image coordinates
- [ ] Long runs show progress and can be cancelled without blocking the app
- [ ] A confidence threshold filters pre-annotations, and I can accept, edit or delete them individually or in bulk
- [ ] The container can be run with ROCm GPU devices passed through by Podman
- [ ] API tests run the full flow against a fake worker that implements the same contract

**Decisions (2026-10-17):**
- The worker only implements an interface, so anything can run: a registered model names an *adapter* (the bundled `ultralytics` one, or a Python file of the user's, e.g. for DefectNet with `prelabel.py`'s shape rules), its weights and options. An adapter loads a model and turns an image into predictions (class name, confidence, rtag shape in image pixels).
- Accepting a pre-annotation marks it reviewed; it keeps its source and confidence. Unreviewed pre-annotations are left out of exports.
- The server starts the worker itself when given a command (`RTAG_WORKER_COMMAND`, e.g. a venv's Python running rtag's worker), else it uses a worker started elsewhere at `RTAG_WORKER_URL`: a container cannot start a process on the host, so a containerised server talks to a worker running on the host (or in its own GPU container). This relaxes ADR 0001's "in the same image".
- Running a model again on an image replaces that model's unreviewed pre-annotations there; reviewed ones and manual work stay.
- Images go to the worker as bytes (not paths), so it works when server and worker see different file systems.
