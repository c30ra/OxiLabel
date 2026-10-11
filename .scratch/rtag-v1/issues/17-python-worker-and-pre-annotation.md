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
