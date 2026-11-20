# Runtime bundles

`scripts/build_runtime.py` creates a local runtime bundle for one of the six
native Rust targets in `bindings/runtime/targets.json`. The build needs the Rust
toolchain, CMake, and the native C/C++ compiler; consumer machines need only the
runtime bundle and their language SDK.

```powershell
rtk uv run --with-requirements scripts/requirements-bindings.txt python -m unittest discover -s scripts/tests -p test_runtime_bundle.py -v
rtk proxy python scripts/build_runtime.py --target host --out target/runtime/current
target\runtime\current\bin\grafikr-host.exe --version
```

`--target host` accepts only a host triple listed in the target matrix. Building
another triple is allowed only on a native runner for that triple. A successful
bundle contains `manifest.json`, the host, C ABI libraries, C/C++ headers and
libraries, CMake config, six C/C++ example executables under
`share/grafikr/examples/`, and the Grafikr license. The manifest records the
engine, protocol and ABI versions, target minima, build provenance, quotas,
external system requirements, and the SHA-256 and size of every shipped file.
The Windows bundle requires the Microsoft Visual C++ 2015-2022 Redistributable
matching its architecture.

`scripts.build_runtime.validate_manifest` checks the schema, target minima,
safe relative paths, regular-file status, link count, file sizes and hashes, and
the host and native example executables' actual OS and architecture. `install_bundle` validates
before copying only listed files into a temporary sibling directory; it
validates the staged copy before publishing it. It will reuse an identical
verified destination and refuses to replace a different existing cache.

`--development-fixture` includes a separate `fixture-host` executable backed
by the real dispatcher and a CPU `Ui`; it does not open a native window and
does not prove GUI behavior. The manifest marks this bundle for development,
and release packaging must reject marked bundles. A locally built bundle is not evidence that the target works on a
different OS version, display backend, architecture, or GPU. Linux artifacts
must be built on Ubuntu 22.04 for the declared glibc baseline. Windows DLL and
macOS/Linux dynamic loader dependencies still require platform-specific audit
before release.

Registry publication, platform signing/notarization, and public availability
are separate release steps. Do not describe local bundles as published SDKs.
