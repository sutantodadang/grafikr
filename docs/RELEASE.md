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
