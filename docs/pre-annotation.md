# Pre-annotation: running the worker

rtag's server sends images to a Python worker (`worker/`, ADR 0001) that runs models through
adapters and returns pre-annotations. The worker needs the model's Python stack (e.g. PyTorch for
ROCm and Ultralytics); the server only needs to reach it over HTTP.

An admin sets where the worker is on the **Settings** screen: a start command, a URL, or both.
Saving applies it at once; the Settings and Models screens show whether the worker answers and
on which device.

## Server and worker on the same machine

Give the start command, with the Python that has the model's stack:

    PYTHONPATH=/path/to/rtag/worker exec /path/to/venv/bin/python -m rtag_worker

The server runs it through the shell, tells it to listen at the URL (default
`http://127.0.0.1:3112`), starts it again if it exits, and stops it (with anything it started)
when the settings change or the server stops.

## Server in a container, worker on the host

A container cannot start processes on the host, so start the worker there yourself:

    PYTHONPATH=worker /path/to/venv/bin/python -m rtag_worker --addr 0.0.0.0:3112

and on the Settings screen leave the command empty and set the URL to
`http://host.containers.internal:3112`. Keep that port closed to other machines (see Trust).

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
