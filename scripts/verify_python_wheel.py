"""Build, offline-install, and smoke-test a Python wheel outside its source tree."""

from __future__ import annotations

import argparse
import os
import subprocess
import tempfile
import venv
from pathlib import Path

from scripts.package_python import package


PYTHON_INSTALLED_SCENARIOS = r'''
import base64
import os
import threading
from pathlib import Path

from grafikr import App, CollectionMetadata, CollectionProvider, Node, State, Window
import grafikr.client

fixture = Path(os.environ["GRAFIKR_FIXTURE_HOST"])
grafikr.client.runtime_host_path = lambda _bundle=None: fixture

def counter_form():
    app = App()
    try:
        window = Window.create(app, "Installed counter", 480, 320)
        count = State.create(app, 0)
        button = Node.create(app, window, "Button", label="Increment")
        label = Node.create(app, window, "Text", text="Count: 0")
        field = Node.create(app, window, "TextField", text="Grafikr 雪", width=240)
        button.attach(window)
        label.attach(window)
        field.attach(window)
        handled = threading.Event()
        def increment(_event):
            value = count.get() + 1
            count.set(value)
            label.update(text=f"Count: {value}")
            handled.set()
        subscription = app.on(button.handle, "click", increment)
        assert handled.wait(5), "installed counter did not receive its click"
        assert count.get() == 1, "installed counter state did not update"
        field.update(text="Input 雪")
        subscription.close()
        count.close()
        window.close()
    finally:
        app.close()

def dashboard():
    app = App()
    try:
        window = Window.create(app, "Installed dashboard", 640, 480)
        rows = Node.create(app, window, "VirtualList", row_count=50000, row_height=24, width=600, height=360)
        rows.attach(window)
        provider = CollectionProvider.create(rows, CollectionMetadata(50000, 24, ["label"]))
        request = provider.next_request(timeout=5)
        assert request is not None and request.start == 0 and request.count == 32
        provider.provide(request, [[f"Row {index}"] for index in range(request.count)])

        canvas = Node.create(app, window, "Canvas", commands=[], label="Dashboard canvas", width=320, height=120)
        canvas.attach(window)
        app.request("canvas.update", {"node": canvas.handle, "commands": [{
            "kind": "rect",
            "rect": {"x": 0.0, "y": 0.0, "width": 32.0, "height": 24.0},
            "color": "#336699",
            "radius": 0.0,
        }]})
        upload = app.request("image.begin", {"width": 1, "height": 1, "byte_length": 4})["upload"]
        app.request("image.chunk", {"upload": upload, "offset": 0, "data": base64.b64encode(bytes((0, 0, 0, 255))).decode("ascii")})
        image = app.request("image.finalize", {"upload": upload})["image"]
        image_node = Node.create(app, window, "Image", image=image, width=1, height=1, label="Pixel")
        image_node.attach(window)
        app.request("image.destroy", {"image": image})
        image_node.destroy()
        canvas.destroy()
        provider.close()
        rows.destroy()
        window.close()
    finally:
        app.close()

def multi_window():
    app = App()
    try:
        first = Window.create(app, "First window", 320, 240)
        second = Window.create(app, "Second window", 320, 240)
        modal = Node.create(app, second, "Modal", open=True, width=240)
        modal.attach(second)
        message = Node.create(app, second, "Text", text="Confirm")
        message.attach(modal)
        state = State.create(app, 1)
        first.close()
        state.set(2)
        assert state.get() == 2, "remaining window lost session state"
        message.destroy()
        modal.destroy()
        state.close()
        second.close()
    finally:
        app.close()

counter_form()
dashboard()
multi_window()
print("installed Python counter_form, dashboard, and multi_window passed")
'''


def _run(argv: list[str], cwd: Path, environment: dict[str, str]) -> str:
    result = subprocess.run(
        argv,
        cwd=cwd,
        env=environment,
        shell=False,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=120,
        check=False,
    )
    if result.returncode:
        raise RuntimeError(f"command failed ({result.returncode}): {argv!r}\n{(result.stdout + result.stderr)[-4000:]}")
    return result.stdout


def verify(bundle: Path, fixture_scenarios: bool = False) -> str:
    with tempfile.TemporaryDirectory(prefix="grafikr-python-wheel-consumer-") as temporary:
        root = Path(temporary)
        wheel_dir = root / "wheels"
        wheel_dir.mkdir()
        wheel = package(bundle, wheel_dir)
        environment_dir = root / "consumer-venv"
        venv.EnvBuilder(with_pip=True).create(environment_dir)
        python = environment_dir / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
        child_environment = os.environ.copy()
        child_environment.pop("PYTHONPATH", None)
        child_environment.pop("PYTHONHOME", None)
        _run([str(python), "-m", "pip", "install", "--no-index", "--no-deps", str(wheel)], root, child_environment)
        _run(
            [
                str(python),
                "-c",
                "import subprocess; subprocess.Popen=lambda *a,**k: (_ for _ in ()).throw(RuntimeError('process on import')); import grafikr; print('import safe')",
            ],
            root,
            child_environment,
        )
        if fixture_scenarios:
            fixture = child_environment.get("GRAFIKR_FIXTURE_HOST")
            if not fixture or not Path(fixture).is_file():
                raise ValueError("installed fixture scenarios require GRAFIKR_FIXTURE_HOST")
            scenario_script = root / "installed_scenarios.py"
            scenario_script.write_text(PYTHON_INSTALLED_SCENARIOS, encoding="utf-8")
            scenario_environment = child_environment | {
                "GRAFIKR_FIXTURE_EARLY_SUBSCRIBE": "1",
                "GRAFIKR_FIXTURE_COLLECTION_REQUEST": "1",
            }
            _run([str(python), str(scenario_script)], root, scenario_environment)
        _run(
            [
                str(python),
                "-c",
                "import sys; from pathlib import Path; from grafikr import App, State, runtime_host_path; package=Path(__import__('grafikr').__file__).resolve().parents[1]; host=runtime_host_path(); assert host.is_relative_to(package); app=App(); state=State.create(app, -(2**63)); assert state.get()==-(2**63); app.close(); print(host)",
            ],
            root,
            child_environment,
        )
    return wheel.name


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--fixture-scenarios", action="store_true")
    args = parser.parse_args(argv)
    try:
        wheel_name = verify(args.bundle.resolve(), args.fixture_scenarios)
    except (OSError, ValueError, RuntimeError, subprocess.TimeoutExpired) as error:
        parser.exit(1, f"Python installed-wheel verification failed: {error}\n")
    print(f"Python installed wheel verified: {wheel_name}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
