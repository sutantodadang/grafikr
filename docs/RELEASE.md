# Release Checklist

Used for 1.0.x releases and later.

## Before tagging

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace`
4. `cargo run -p grafikr-bench --release`
5. Run the `settings`, `dialog`, `combo-demo`, `datagrid-demo`, and
   `multi-window` examples on Windows and at least one other platform.
6. Check golden images: use `GRAFIKR_UPDATE_GOLDEN=1` only for intentional
   visual changes, then review every changed PNG.
7. Update `CHANGELOG.md`, `docs/STATUS.md`, and the version in `Cargo.toml`.
8. Confirm the README and tutorial still match the API.

## Cross-language SDK release gate

Cross-language packages are not released by the Rust crate checklist above.
Before claiming full SDK/platform support or publishing any language package:

1. Build and validate all six native runtime bundles listed in
   `bindings/runtime/targets.json`; record target, source SHA, manifest hash,
   OS baseline, and external system requirements.
2. Run the complete package builder and verifier:

   ```powershell
   rtk uv run --python 3.12 --with-requirements scripts/requirements-bindings.txt python -m scripts.package_bindings --version 1.1.0 --runtime-root target/runtime/releases --out target/packages/release
   rtk uv run --python 3.12 --with-requirements scripts/requirements-bindings.txt python -m scripts.package_bindings --version 1.1.0 --runtime-root target/runtime/releases --out target/packages/release --check
   ```

   Stop if the index is source-dirty, incomplete, or a package checksum does not
   validate. The local source tree must be committed before producing release
   artifacts; rebuild after any source change that affects an artifact.
3. Run unit and protocol/widget contract suites for all SDK rows in
   `bindings/sdk-matrix.json`. The shared protocol cases and widget/property/
   event inventory must be covered, with only explicit, justified per-SDK
   exclusions.
4. Run the installed-package counter/form, dashboard, and multi-window GUI
   scenarios for each SDK and required display cell using real OS input. Add
   separate native hardware, GPU, accessibility, and benchmark evidence for all
   eight display cells. Synthetic `UiEvent` injection is not GUI evidence.
5. Validate the evidence gate:

   ```powershell
   rtk uv run --python 3.12 --with-requirements scripts/requirements-bindings.txt python -m scripts.check_bindings_evidence --root .artifacts/bindings --mode full --out .artifacts/bindings/release-gate.json
   ```

   A local subset must remain labeled `LOCAL_ONLY`; do not infer full support
   from source builds, headless CTest, cross-compilation, or a subset of SDKs.
6. Publish registry packages, sign/notarize platform binaries, and announce
   availability only as separate release actions after their respective
   credentials and platform requirements are ready. Local package archives do
   not mean the SDK is publicly available.

## Publishing to crates.io

The order matters because path + version dependencies must already exist in the
registry:

```bash
cargo publish -p grafikr-core
cargo publish -p grafikr-render
cargo publish -p grafikr-widgets
cargo publish -p grafikr-shell
cargo publish -p grafikr
```

After publishing, test a new project that depends only on `grafikr = "1.0"`.

## After tagging

- Verify CI on the tag.
- Archive screenshots and benchmarks for the release.
- Open issues for the gaps recorded in `docs/STATUS.md`.
