"""Install the Go SDK from a local module proxy and exercise its runtime installer."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import tempfile
import tomllib
import zipfile
from datetime import datetime, timezone
from pathlib import Path

from scripts.build_runtime import ROOT, validate_manifest

MODULE = "github.com/grafikr/grafikr-go"
VERSION = f"v{tomllib.loads((ROOT / 'Cargo.toml').read_text(encoding='utf-8'))['workspace']['package']['version']}"
SDK = ROOT / "bindings/go"


def create_proxy(sdk: Path, proxy: Path, license_path: Path, version: str = VERSION) -> Path:
    module_cache = proxy / MODULE / "@v"
    module_cache.mkdir(parents=True, exist_ok=True)
    archive = module_cache / f"{version}.zip"
    prefix = f"{MODULE}@{version}/"
    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as package:
        for path in sorted(sdk.rglob("*")):
            if path.is_symlink():
                raise ValueError(f"Go module contains a symlink: {path.relative_to(sdk)}")
            if not path.is_file():
                continue
            relative = path.relative_to(sdk).as_posix()
            if relative.startswith(("target/", ".git/")):
                continue
            package.write(path, prefix + relative)
        package.write(license_path, prefix + "LICENSE")
    mod = (sdk / "go.mod").read_text(encoding="utf-8")
    timestamp = datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")
    (module_cache / f"{version}.mod").write_text(mod, encoding="utf-8")
    (module_cache / f"{version}.info").write_text(
        json.dumps({"Version": version, "Time": timestamp}) + "\n",
        encoding="utf-8",
    )
    (module_cache / "list").write_text(f"{version}\n", encoding="utf-8")
    return archive


def export_proxy(proxy: Path, destination: Path) -> None:
    source = proxy / MODULE / "@v"
    output = destination / MODULE / "@v"
    output.mkdir(parents=True, exist_ok=True)
    for artifact in source.iterdir():
        if artifact.is_file():
            shutil.copy2(artifact, output / artifact.name)


def verify(
    bundle: Path,
    output: Path | None = None,
    version: str = VERSION,
    fixture_scenarios: bool = False,
) -> None:
    bundle = bundle.resolve()
    manifest = validate_manifest(bundle)
    if manifest["development_fixture"]:
        raise ValueError("development fixture bundles cannot be installed by Go consumers")
    if manifest["engine_version"] != version.removeprefix("v"):
        raise ValueError("Go module and runtime engine versions do not match")

    with tempfile.TemporaryDirectory(prefix="grafikr-go-consumer-") as temporary:
        work = Path(temporary)
        proxy = work / "proxy"
        create_proxy(SDK, proxy, ROOT / "LICENSE", version)
        consumer = work / "consumer"
        consumer.mkdir()
        (consumer / "go.mod").write_text(
            f"module external-consumer\n\ngo 1.26\n\nrequire {MODULE} {version}\n",
            encoding="utf-8",
        )
        (consumer / "main.go").write_text(
            '''package main
import (
  "encoding/json"
  "fmt"
  "os"
  "strconv"
  "time"
  "github.com/grafikr/grafikr-go"
)
func main() {
  app, err := grafikr.StartWithRuntime(os.Args[1], os.Args[2])
  if err != nil { panic(err) }
  defer app.Close()
  state, err := app.CreateState(map[string]any{"type":"i64", "value":"-9223372036854775808"})
  if err != nil { panic(err) }
  raw, err := state.Get()
  if err != nil { panic(err) }
  var value struct { Type string `json:"type"`; Value string `json:"value"` }
  if err := json.Unmarshal(raw, &value); err != nil { panic(err) }
  if value.Type != "i64" || value.Value != "-9223372036854775808" { panic(fmt.Sprintf("minimum mismatch: %+v", value)) }
  if err := state.Set(map[string]any{"type":"i64", "value":"9223372036854775807"}); err != nil { panic(err) }
  raw, err = state.Get()
  if err != nil { panic(err) }
  if err := json.Unmarshal(raw, &value); err != nil { panic(err) }
  if value.Type != "i64" || value.Value != "9223372036854775807" { panic(fmt.Sprintf("maximum mismatch: %+v", value)) }
  if err := state.Close(); err != nil { panic(err) }
  if os.Getenv("GRAFIKR_FIXTURE_SCENARIOS") == "1" { runScenarios() }
}
func runScenarios() {
  fixture := os.Getenv("GRAFIKR_FIXTURE_HOST")
  if fixture == "" { panic("GRAFIKR_FIXTURE_HOST is required") }
  app, err := grafikr.Start(fixture); if err != nil { panic(err) }; defer app.Close()
  closeWindow := func(window grafikr.Window) { if err := app.Request("window.close", map[string]any{"window":window.Handle}, nil); err != nil { panic(err) } }

  window, err := app.CreateWindow("Installed counter", 480, 320); if err != nil { panic(err) }
  count, err := app.CreateState(map[string]any{"type":"i64", "value":"0"}); if err != nil { panic(err) }
  button, err := app.CreateNode(window, "Button", map[string]any{"label":"Increment"}); if err != nil { panic(err) }
  label, err := app.CreateNode(window, "Text", map[string]any{"text":"Count: 0"}); if err != nil { panic(err) }
  field, err := app.CreateNode(window, "TextField", map[string]any{"text":"Grafikr 雪", "width":240}); if err != nil { panic(err) }
  for _, node := range []grafikr.Node{button, label, field} { if err := node.Attach(window.Root); err != nil { panic(err) } }
  clicked := make(chan struct{}, 1)
  subscription, err := app.Subscribe(button.Handle, "click", func(grafikr.Event) {
    raw, err := count.Get(); if err != nil { panic(err) }
    var value struct { Value string `json:"value"` }; if err := json.Unmarshal(raw, &value); err != nil { panic(err) }
    next, err := strconv.ParseInt(value.Value, 10, 64); if err != nil { panic(err) }
    if err := count.Set(map[string]any{"type":"i64", "value":strconv.FormatInt(next+1,10)}); err != nil { panic(err) }
    if err := label.Update(map[string]any{"text":"Count: 1"}); err != nil { panic(err) }
    clicked <- struct{}{}
  }); if err != nil { panic(err) }
  select { case <-clicked: case <-time.After(5*time.Second): panic("counter click timed out") }
  raw, err := count.Get(); if err != nil { panic(err) }
  var countValue struct { Value string `json:"value"` }; if err := json.Unmarshal(raw,&countValue); err != nil || countValue.Value != "1" { panic("counter state mismatch") }
  if err := field.Update(map[string]any{"text":"Input 雪"}); err != nil { panic(err) }
  if err := app.Unsubscribe(subscription); err != nil { panic(err) }
  if err := count.Close(); err != nil { panic(err) }; closeWindow(window)

  dashboard, err := app.CreateWindow("Installed dashboard",640,480); if err != nil { panic(err) }
  rows, err := app.CreateNode(dashboard,"VirtualList",map[string]any{"row_count":50000,"row_height":24,"width":600,"height":360}); if err != nil { panic(err) }
  if err := rows.Attach(dashboard.Root); err != nil { panic(err) }
  provider, err := app.CreateCollectionProvider(rows,grafikr.CollectionMetadata{RowCount:50000,RowHeight:24,Columns:[]string{"label"}}); if err != nil { panic(err) }
  var request grafikr.CollectionRequest
  select { case request = <-provider.Requests(): case <-time.After(5*time.Second): panic("collection request timed out") }
  if request.Count != 32 || request.Start != 0 { panic("visible range mismatch") }
  values := make([][]any, request.Count); for index := range values { values[index] = []any{fmt.Sprintf("Row %d",index)} }
  if err := provider.Provide(request,values); err != nil { panic(err) }
  canvas, err := app.CreateCanvas(dashboard,map[string]any{"commands":[]any{},"label":"Dashboard canvas","width":320,"height":120}); if err != nil { panic(err) }
  if err := canvas.Attach(dashboard.Root); err != nil { panic(err) }
  if err := canvas.Update([]map[string]any{{"kind":"rect","rect":map[string]any{"x":0.0,"y":0.0,"width":32.0,"height":24.0},"color":"#336699","radius":0.0}}); err != nil { panic(err) }
  image, err := app.UploadImage(1,1,[]byte{0,0,0,255}); if err != nil { panic(err) }
  imageNode, err := app.CreateNode(dashboard,"Image",map[string]any{"image":image.Handle,"width":1,"height":1,"label":"Pixel"}); if err != nil { panic(err) }
  if err := imageNode.Attach(dashboard.Root); err != nil { panic(err) }
  if err := image.Destroy(); err != nil { panic(err) }
  if err := imageNode.Destroy(); err != nil { panic(err) }; if err := canvas.Destroy(); err != nil { panic(err) }
  if err := provider.Close(); err != nil { panic(err) }; if err := rows.Destroy(); err != nil { panic(err) }
  closeWindow(dashboard)

  first, err := app.CreateWindow("First window",320,240); if err != nil { panic(err) }
  second, err := app.CreateWindow("Second window",320,240); if err != nil { panic(err) }
  modal, err := app.CreateNode(second,"Modal",map[string]any{"open":true,"width":240}); if err != nil { panic(err) }
  if err := modal.Attach(second.Root); err != nil { panic(err) }
  message, err := app.CreateNode(second,"Text",map[string]any{"text":"Confirm"}); if err != nil { panic(err) }
  if err := message.Attach(modal.Handle); err != nil { panic(err) }
  shared, err := app.CreateState(map[string]any{"type":"i64","value":"1"}); if err != nil { panic(err) }
  closeWindow(first)
  if err := shared.Set(map[string]any{"type":"i64","value":"2"}); err != nil { panic(err) }
  raw, err = shared.Get(); if err != nil { panic(err) }
  var sharedValue struct { Value string `json:"value"` }; if err := json.Unmarshal(raw,&sharedValue); err != nil || sharedValue.Value != "2" { panic("remaining window state mismatch") }
  if err := message.Destroy(); err != nil { panic(err) }; if err := modal.Destroy(); err != nil { panic(err) }
  if err := shared.Close(); err != nil { panic(err) }; closeWindow(second)
}
''',
            encoding="utf-8",
        )
        if fixture_scenarios:
            environment = os.environ.copy()
            if not environment.get("GRAFIKR_FIXTURE_HOST"):
                raise ValueError("installed fixture scenarios require GRAFIKR_FIXTURE_HOST")
            environment["GRAFIKR_FIXTURE_SCENARIOS"] = "1"
        else:
            environment = os.environ.copy()
        environment.update(
            GOPROXY=proxy.as_uri(),
            GOSUMDB="off",
            GOPATH=str(work / "gopath"),
            GOMODCACHE=str(work / "gomodcache"),
            GOCACHE=str(work / "gocache"),
            GOTOOLCHAIN="local",
        )
        downloaded = subprocess.run(
            ["go", "mod", "tidy"],
            cwd=consumer,
            env=environment,
            shell=False,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=300,
            check=False,
        )
        if downloaded.returncode:
            raise RuntimeError(
                f"external Go module install failed ({downloaded.returncode}): "
                f"{downloaded.stderr[-2000:]}"
            )
        completed = subprocess.run(
            ["go", "run", ".", str(bundle), str(work / "runtime-cache")],
            cwd=consumer,
            env=environment,
            shell=False,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=300,
            check=False,
        )
        if completed.returncode:
            raise RuntimeError(
                f"external Go consumer failed ({completed.returncode}): "
                f"{completed.stderr[-2000:]}"
            )
        if output is not None:
            export_proxy(proxy, output.resolve())
    print("External Go module consumer installed offline and passed runtime/i64 IPC checks")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--out", type=Path)
    parser.add_argument("--version", default=VERSION)
    parser.add_argument("--fixture-scenarios", action="store_true")
    args = parser.parse_args(argv)
    try:
        verify(args.bundle, args.out, args.version, args.fixture_scenarios)
    except (OSError, ValueError, RuntimeError, zipfile.BadZipFile) as error:
        parser.exit(1, f"Go package verification failed: {error}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
