"""Install the native Ruby and PHP artifacts from a package-index in clean consumers."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import tempfile
from pathlib import Path
from typing import Any

from scripts.build_runtime import ROOT, _host_triple, _tool
from scripts.package_bindings import _package_path, _php_runtime_slug, validate_package_index


def _artifact(root: Path, index: dict[str, Any], sdk: str, target: str) -> Path:
    matches = [
        entry for entry in index.get("packages", [])
        if entry.get("sdk") == sdk and entry.get("target") == target
    ]
    if len(matches) != 1:
        raise ValueError(f"package index must contain exactly one {sdk}/{target} artifact")
    artifact = root.joinpath(*_package_path(matches[0].get("path")).parts)
    if not artifact.is_file() or artifact.is_symlink():
        raise ValueError(f"package artifact is missing or unsafe: {artifact}")
    return artifact


def _with_environment(values: dict[str, str], action) -> None:
    previous = {name: os.environ.get(name) for name in values}
    os.environ.update(values)
    try:
        action()
    finally:
        for name, value in previous.items():
            if value is None:
                os.environ.pop(name, None)
            else:
                os.environ[name] = value


def _php_command() -> list[str]:
    executable = os.environ.get("GRAFIKR_PHP_BINARY", "php")
    if executable != "php" and (not Path(executable).is_absolute() or not Path(executable).is_file()):
        raise ValueError("GRAFIKR_PHP_BINARY must be an existing absolute file")
    command = [executable]
    extensions = [
        name for name in ("GRAFIKR_PHP_OPENSSL_EXTENSION", "GRAFIKR_PHP_ZIP_EXTENSION")
        if os.environ.get(name)
    ]
    for name in extensions:
        extension = Path(os.environ[name])
        if not extension.is_absolute() or not extension.is_file():
            raise ValueError(f"{name} must be an existing absolute file")
        module = "openssl" if "OPENSSL" in name else "zip"
        if len(command) == 1:
            command.extend(("-d", f"extension_dir={extension.parent}"))
        command.extend(("-d", f"extension={module}"))
    return command


def verify(package_root: Path, fixture_scenarios: bool = False, sdk: str = "all") -> None:
    if sdk not in {"all", "ruby", "php"}:
        raise ValueError("sdk must be all, ruby, or php")
    package_root = Path(package_root).resolve()
    index = json.loads((package_root / "package-index.json").read_text(encoding="utf-8"))
    if not isinstance(index, dict) or not isinstance(index.get("version"), str):
        raise ValueError("package index is missing its release version")
    if fixture_scenarios:
        if index.get("format") != "grafikr-bindings-package-index-v1":
            raise ValueError("unsupported local package index schema")
    else:
        validate_package_index(index, package_root, index["version"])
    target = _host_triple()
    artifacts = []
    gem = _artifact(package_root, index, "ruby", target) if sdk in {"all", "ruby"} else None
    php_source = _artifact(package_root, index, "php", "source") if sdk in {"all", "php"} else None
    php_runtime = _artifact(package_root, index, "php", target) if sdk in {"all", "php"} else None
    if gem:
        artifacts.append(("ruby", gem))
    if php_source:
        artifacts.extend((("php", php_source), ("php", php_runtime)))
    if fixture_scenarios:
        for package_sdk, artifact in artifacts:
            record = next(row for row in index["packages"] if row.get("sdk") == package_sdk and row.get("path") and
                          package_root.joinpath(*_package_path(row["path"]).parts) == artifact)
            digest = hashlib.sha256(artifact.read_bytes()).hexdigest()
            if digest != record.get("sha256"):
                raise ValueError(f"local {package_sdk} package checksum does not match its index")
        fixture = os.environ.get("GRAFIKR_FIXTURE_HOST")
        if not fixture or not Path(fixture).is_absolute() or not Path(fixture).is_file():
            raise ValueError("local installed scenarios require an absolute GRAFIKR_FIXTURE_HOST")
    (ROOT / "target").mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="grafikr-package-consumers-", dir=ROOT / "target") as temporary:
        root = Path(temporary)
        gem_home = root / "gem-home"
        gem_home.mkdir()
        environment = {"GEM_HOME": str(gem_home), "GEM_PATH": str(gem_home)}
        if sdk in {"all", "ruby"}:
            _with_environment(
                environment,
                lambda: _tool("gem", "install", str(gem), "--local", "--no-document", "--install-dir", str(gem_home)),
            )
            ruby_program = 'require "grafikr"; app = Grafikr::Client.start'
            if fixture_scenarios:
                ruby_program = '''
require "grafikr"
app = Grafikr::Client.start(host: ENV.fetch("GRAFIKR_FIXTURE_HOST"))
begin
  window = app.create_window("Counter", 480, 320)
  count = app.create_state({"type" => "i64", "value" => "0"})
  button = app.create_node(window, "Button", {"label" => "Increment"})
  label = app.create_node(window, "Text", {"text" => "Count: 0"})
  field = app.create_node(window, "TextField", {"text" => "Grafikr 雪", "width" => 240})
  [button, label, field].each { |node| node.attach(window.root) }
  handled = Queue.new
  subscription = app.on(button.handle, "click") do
    next_value = count.get.fetch("value").to_i + 1
    count.set({"type" => "i64", "value" => next_value.to_s})
    label.update({"text" => "Count: #{next_value}"})
    handled << true
  end
  raise "counter click timed out" unless Timeout.timeout(5) { handled.pop }
  raise "counter state did not update" unless count.get.fetch("value") == "1"
  field.update({"text" => "Input 雪"})
  subscription.close; count.close; window.close

  dashboard = app.create_window("Dashboard", 640, 480)
  rows = app.create_node(dashboard, "VirtualList", {"row_count" => 50000, "row_height" => 24, "width" => 600, "height" => 360})
  rows.attach(dashboard.root)
  provider = app.create_collection_provider(rows, row_count: 50000, row_height: 24, columns: ["label"])
  request = provider.next_request(timeout: 5)
  raise "collection request missing" unless request && request.start == 0 && request.count == 32
  provider.provide(request, Array.new(request.count) { |index| ["Row #{index}"] })
  canvas = app.create_node(dashboard, "Canvas", {"commands" => [], "label" => "Dashboard canvas", "width" => 320, "height" => 120})
  canvas.attach(dashboard.root)
  app.request("canvas.update", {"node" => canvas.handle, "commands" => [{"kind" => "rect", "rect" => {"x" => 0.0, "y" => 0.0, "width" => 32.0, "height" => 24.0}, "color" => "#336699", "radius" => 0.0}]})
  upload = app.request("image.begin", {"width" => 1, "height" => 1, "byte_length" => 4}).fetch("upload")
  app.request("image.chunk", {"upload" => upload, "offset" => 0, "data" => [0, 0, 0, 255].pack("C*").then { |data| [data].pack("m0") }})
  image = app.request("image.finalize", {"upload" => upload}).fetch("image")
  image_node = app.create_node(dashboard, "Image", {"image" => image, "width" => 1, "height" => 1, "label" => "Pixel"})
  image_node.attach(dashboard.root); app.request("image.destroy", {"image" => image})
  image_node.destroy; canvas.destroy; provider.close; rows.destroy; dashboard.close

  first = app.create_window("First", 320, 240); second = app.create_window("Second", 320, 240)
  modal = app.create_node(second, "Modal", {"open" => true, "width" => 240}); modal.attach(second.root)
  message = app.create_node(second, "Text", {"text" => "Confirm"}); message.attach(modal.handle)
  shared = app.create_state({"type" => "i64", "value" => "1"})
  first.close; shared.set({"type" => "i64", "value" => "2"})
  raise "remaining window lost session state" unless shared.get.fetch("value") == "2"
  message.destroy; modal.destroy; shared.close; second.close
ensure
  app.close
end
'''
            _with_environment(environment, lambda: _tool("ruby", "-e", ruby_program, cwd=root))

        if sdk in {"all", "php"}:
            composer_project = root / "php"
            composer_project.mkdir()
            slug = _php_runtime_slug(target)
            composer = {
                "name": "grafikr/package-consumer",
                "version": index["version"],
                "repositories": [
                    {"type": "artifact", "url": str(package_root / "php")},
                    {"packagist.org": False},
                ],
                "require": {"grafikr/ui": index["version"], f"grafikr/runtime-{slug}": index["version"]},
                "config": {"allow-plugins": False},
            }
            (composer_project / "composer.json").write_text(json.dumps(composer, indent=2) + "\n", encoding="utf-8")
            composer_phar = os.environ.get("GRAFIKR_COMPOSER_PHAR")
            if composer_phar:
                phar = Path(composer_phar)
                if not phar.is_absolute() or not phar.is_file():
                    raise ValueError("GRAFIKR_COMPOSER_PHAR must be an existing absolute file")
                composer_command = _php_command() + [str(phar)]
            else:
                composer_command = ["composer"]
            _tool(
                *composer_command, "install", "--no-interaction", "--no-scripts", "--no-plugins", "--no-progress",
                cwd=composer_project,
            )
            php_program = 'require "vendor/autoload.php"; $app = Grafikr\\Client::start();'
            if fixture_scenarios:
                php_program = '''
require "vendor/autoload.php";
$app = Grafikr\\Client::start(new Grafikr\\RuntimeOptions(hostPath: getenv("GRAFIKR_FIXTURE_HOST")));
try {
  $window = $app->createWindow("Counter", 480, 320);
  $count = $app->createState(["type" => "i64", "value" => "0"]);
  $button = $app->createNode($window, "Button", ["label" => "Increment"]);
  $label = $app->createNode($window, "Text", ["text" => "Count: 0"]);
  $field = $app->createNode($window, "TextField", ["text" => "Grafikr 雪", "width" => 240]);
  foreach ([$button, $label, $field] as $node) $node->attach($window->root);
  $clicked = false;
  $subscription = $app->on($button->handle, "click", function () use (&$clicked, $count, $label): void {
    $value = (int)$count->get()["value"] + 1;
    $count->set(["type" => "i64", "value" => (string)$value]);
    $label->update(["text" => "Count: $value"]);
    $clicked = true;
  });
  if (!$clicked || $count->get()["value"] !== "1") throw new RuntimeException("counter click/state failed");
  $field->update(["text" => "Input 雪"]); $subscription->close(); $count->close(); $window->close();

  $dashboard = $app->createWindow("Dashboard", 640, 480);
  $rows = $app->createNode($dashboard, "VirtualList", ["row_count" => 50000, "row_height" => 24, "width" => 600, "height" => 360]);
  $rows->attach($dashboard->root);
  $provider = $rows->createCollectionProvider(["row_count" => 50000, "row_height" => 24, "columns" => ["label"]]);
  $request = $provider->nextRequest(5.0);
  if (!$request || $request->start !== 0 || $request->count !== 32) throw new RuntimeException("collection request missing");
  $provider->provide($request, array_map(fn($i) => ["Row $i"], range(0, $request->count - 1)));
  $canvas = $app->createNode($dashboard, "Canvas", ["commands" => [], "label" => "Dashboard canvas", "width" => 320, "height" => 120]);
  $canvas->attach($dashboard->root);
  $app->request("canvas.update", ["node" => $canvas->handle, "commands" => [["kind" => "rect", "rect" => ["x" => 0.0, "y" => 0.0, "width" => 32.0, "height" => 24.0], "color" => "#336699", "radius" => 0.0]]]);
  $upload = $app->request("image.begin", ["width" => 1, "height" => 1, "byte_length" => 4])["upload"];
  $app->request("image.chunk", ["upload" => $upload, "offset" => 0, "data" => base64_encode("\\0\\0\\0\\xff")]);
  $image = $app->request("image.finalize", ["upload" => $upload])["image"];
  $imageNode = $app->createNode($dashboard, "Image", ["image" => $image, "width" => 1, "height" => 1, "label" => "Pixel"]);
  $imageNode->attach($dashboard->root); $app->request("image.destroy", ["image" => $image]);
  $imageNode->destroy(); $canvas->destroy(); $provider->close(); $rows->destroy(); $dashboard->close();

  $first = $app->createWindow("First", 320, 240); $second = $app->createWindow("Second", 320, 240);
  $modal = $app->createNode($second, "Modal", ["open" => true, "width" => 240]); $modal->attach($second->root);
  $message = $app->createNode($second, "Text", ["text" => "Confirm"]); $message->attach($modal->handle);
  $shared = $app->createState(["type" => "i64", "value" => "1"]); $first->close();
  $shared->set(["type" => "i64", "value" => "2"]);
  if ($shared->get()["value"] !== "2") throw new RuntimeException("remaining window lost session state");
  $message->destroy(); $modal->destroy(); $shared->close(); $second->close();
} finally { $app->close(); }
'''
            if fixture_scenarios:
                _with_environment(
                    {"GRAFIKR_FIXTURE_HOST": os.environ["GRAFIKR_FIXTURE_HOST"]},
                    lambda: _tool(*_php_command(), "-r", php_program, cwd=composer_project),
                )
            else:
                _tool(*_php_command(), "-r", php_program, cwd=composer_project)

    print(f"{sdk} package consumer passed for {target}")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package-root", type=Path, required=True)
    parser.add_argument("--fixture-scenarios", action="store_true")
    parser.add_argument("--sdk", choices=("all", "ruby", "php"), default="all")
    args = parser.parse_args(argv)
    try:
        verify(args.package_root, args.fixture_scenarios, args.sdk)
    except (OSError, RuntimeError, ValueError, json.JSONDecodeError) as error:
        parser.exit(1, f"Ruby/PHP package verification failed: {error}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
