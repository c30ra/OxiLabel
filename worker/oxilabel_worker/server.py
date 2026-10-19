"""OxiLabel's pre-annotation worker: runs models for the OxiLabel server over local HTTP/JSON (ADR 0001).

    python -m oxilabel_worker --addr 127.0.0.1:3112

The worker knows nothing about particular models: each model names an *adapter*, a Python module
with three functions (see oxilabel_worker/adapters/__init__.py), either the bundled `ultralytics` one
or the path of a .py file on this machine. Loaded models are kept, one per adapter, weights and
options, and run one at a time.

Contract (all JSON; errors are a non-2xx status with {"error": message}):
    GET  /health                                   -> {"device": "..."}
    POST /classes  {adapter, weights, options}     -> {"classes": [name, ...]}
    POST /predict  {adapter, weights, options, image: base64 bytes of the image file}
                                                   -> {"predictions": [{class, confidence, shape}]}
Shapes are OxiLabel's (oxilabel_worker/shapes.py builds them), in the image's own pixels.
"""
import argparse
import base64
import importlib
import importlib.util
import json
import os
import sys
import threading
import traceback
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

DEFAULT_ADDR = "127.0.0.1:3112"
# Models kept loaded (on the GPU); loading another drops the one used longest ago.
KEPT_MODELS = 2


class BadRequest(Exception):
    """A request the worker cannot take, answered with status 400."""


ADAPTER_FILES = {}


def load_adapter(name):
    """The adapter module: a bundled one by name, or a .py file by its path (loaded once)."""
    if name.endswith(".py"):
        path = Path(name).expanduser().resolve()
        if path not in ADAPTER_FILES:
            if not path.is_file():
                raise BadRequest(f"adapter file {path} not found")
            spec = importlib.util.spec_from_file_location(f"oxilabel_adapter_{len(ADAPTER_FILES)}", path)
            module = importlib.util.module_from_spec(spec)
            # The adapter may import modules next to it, e.g. the model's own code.
            sys.path.insert(0, str(path.parent))
            spec.loader.exec_module(module)
            ADAPTER_FILES[path] = module
        return ADAPTER_FILES[path]
    try:
        return importlib.import_module(f"oxilabel_worker.adapters.{name}")
    except ModuleNotFoundError as err:
        if err.name == f"oxilabel_worker.adapters.{name}":
            raise BadRequest(f"no adapter named {name!r}: give a bundled one or the path of a .py file") from err
        raise


def modified(path):
    """When the file was last changed, or None if it is not a file here (e.g. a model name)."""
    try:
        return os.stat(path).st_mtime_ns
    except OSError:
        return None


class Models:
    """Loaded models by adapter, weights and options, the most recently used last; a model whose
    weights file changed is loaded again. Inference runs one at a time."""

    def __init__(self):
        self.loaded = {}
        self.lock = threading.Lock()

    def get(self, request):
        for key in ("adapter", "weights"):
            if not isinstance(request.get(key), str) or not request[key]:
                raise BadRequest(f"the model needs {key}")
        options = request.get("options") or {}
        key = (request["adapter"], request["weights"], json.dumps(options, sort_keys=True))
        stamp = modified(request["weights"])
        entry = self.loaded.pop(key, None)
        if entry is None or entry[3] != stamp:
            entry = None
            while len(self.loaded) >= KEPT_MODELS:
                self.loaded.pop(next(iter(self.loaded)))
            adapter = load_adapter(request["adapter"])
            entry = (adapter, adapter.load(request["weights"], options), options, stamp)
        self.loaded[key] = entry
        return entry[:3]

    def classes(self, request):
        with self.lock:
            adapter, model, _ = self.get(request)
            return [str(name) for name in adapter.classes(model)]

    def predict(self, request):
        try:
            image = base64.b64decode(request["image"], validate=True)
        except (KeyError, TypeError, ValueError) as err:
            raise BadRequest("the request needs the image, base64 encoded") from err
        with self.lock:
            adapter, model, options = self.get(request)
            return [dict(prediction) for prediction in adapter.predict(model, image, options)]


def device():
    """What models run on, for the health check: the GPU if PyTorch sees one."""
    try:
        import torch
    except ImportError:
        return "cpu (no PyTorch)"
    if torch.cuda.is_available():
        return f"cuda:0 {torch.cuda.get_device_name(0)}"
    return "cpu"


def handler(models):
    class Handler(BaseHTTPRequestHandler):
        def answer(self, status, body):
            data = json.dumps(body).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def do_GET(self):
            if self.path == "/health":
                self.answer(200, {"device": device()})
            else:
                self.answer(404, {"error": f"no {self.path}"})

        def do_POST(self):
            routes = {"/classes": lambda r: {"classes": models.classes(r)}, "/predict": lambda r: {"predictions": models.predict(r)}}
            route = routes.get(self.path)
            if route is None:
                return self.answer(404, {"error": f"no {self.path}"})
            try:
                request = json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))))
                if not isinstance(request, dict):
                    raise BadRequest("the request must be a JSON object")
                self.answer(200, route(request))
            except (BadRequest, json.JSONDecodeError) as err:
                self.answer(400, {"error": str(err)})
            except Exception as err:  # the adapter's failure, told to OxiLabel
                traceback.print_exc()
                self.answer(500, {"error": f"{type(err).__name__}: {err}"})

        def log_message(self, format, *args):
            if os.environ.get("OXILABEL_WORKER_QUIET") != "1":
                super().log_message(format, *args)

    return Handler


def serve(addr):
    host, port = addr.rsplit(":", 1)
    server = ThreadingHTTPServer((host, int(port)), handler(Models()))
    print(f"OxiLabel worker listening on http://{host}:{port} ({device()})", flush=True)
    return server


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--addr", default=os.environ.get("OXILABEL_WORKER_ADDR", DEFAULT_ADDR), help=f"host:port to listen on (default {DEFAULT_ADDR}, or OXILABEL_WORKER_ADDR)")
    args = parser.parse_args(argv)
    serve(args.addr).serve_forever()


if __name__ == "__main__":
    main()
