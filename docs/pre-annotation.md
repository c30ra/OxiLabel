# Pre-annotation: running the worker

rtag's server sends images to a Python worker (`worker/`, ADR 0001) that runs models through
adapters and returns pre-annotations. The worker needs the model's Python stack (e.g. PyTorch for
ROCm and Ultralytics); the server only needs to reach it over HTTP.

## Server and worker on the same machine (development)

Let the server start the worker with the Python that has the model's stack:

    RTAG_WORKER_COMMAND="PYTHONPATH=$PWD/worker exec /path/to/venv/bin/python -m rtag_worker" \
    target/release/rtag-server

It listens on `RTAG_WORKER_URL` (default `http://127.0.0.1:3112`); the server restarts it if it
exits. The Models screen shows whether it answers and on which device.

## Server in a container, worker on the host

A container cannot start processes on the host, so start the worker there yourself and point the
server at it:

    PYTHONPATH=worker /path/to/venv/bin/python -m rtag_worker --addr 0.0.0.0:3112

    podman run -p 3000:3000 -v rtag-data:/data -v /photos:/images:ro \
        -e RTAG_WORKER_URL=http://host.containers.internal:3112 localhost/rtag

Listen on an address the container can reach, and keep the port closed to other machines (the
worker runs whatever adapter file a registered model names).

## Worker in its own container with the GPU

ROCm GPUs are passed to a container as devices:

    podman run -d --name rtag-worker -p 127.0.0.1:3112:3112 \
        --device /dev/kfd --device /dev/dri --group-add keep-groups --security-opt seccomp=unconfined \
        -v $PWD/worker:/worker:ro -v /models:/models:ro -e PYTHONPATH=/worker \
        docker.io/rocm/pytorch:latest sh -c "pip install ultralytics && python -m rtag_worker --addr 0.0.0.0:3112"

Model weights and adapter files are then given by their paths inside that container (`/models/…`).

## Trust

Whoever can register models can make the worker run code: an adapter is a Python file the worker
imports, and PyTorch `.pt` weights are pickles that can run code when loaded. Only admins register
models; give admin accounts only to people you would let run programs on the worker's machine, and
keep the worker's port closed to other machines.

## Models and adapters

An admin registers models on the Models screen: a name (the pre-annotations' source), an adapter
(`ultralytics`, or the path of a `.py` file on the worker's machine), the weights file, options as
JSON, and which label each class becomes. `worker/rtag_worker/adapters/__init__.py` describes the
three functions an adapter provides; `worker/rtag_worker/shapes.py` builds the shapes.
