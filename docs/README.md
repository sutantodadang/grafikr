# Grafikr Documentation

Start with the tutorial, then use the reference material for the task at hand.

| Document | Purpose |
| --- | --- |
| [Tutorial](tutorial.md) | Build your first app, use state, forms, lists, animation, and custom widgets |
| [Roadmap](ROADMAP.md) | Vision, architecture, phases, testing strategy, and risks |
| [Status](STATUS.md) | What is implemented, known gaps, and post-1.0 work |
| [Migration](MIGRATION.md) | API changes for the 1.0 release |
| [Release checklist](RELEASE.md) | Quality gates and crates.io publish order |
| [Cross-language support matrix](bindings/support-matrix.md) | Verified SDK/runtime evidence and open gates |
| [Cross-language runtime installation](bindings/runtime-installation.md) | Local bundle creation and installation contract |
| [Cross-language SDK contract](bindings/sdk-contract.md) | Shared protocol, coverage, and evidence rules |
| [ADRs](adr/) | Architecture decision records: retained tree, signals, renderer, transforms, device recovery |

API reference is generated from Rustdoc:

```bash
cargo doc --workspace --no-deps --open
```

Examples live in [`../examples`](../examples). Start with `counter`, then
`settings`, `dialog`, and `datagrid`.
