# RequestTailor

An API client that belongs to the developer: collections are plain files in your Git repo,
the same Rust core runs natively, in the browser (WebAssembly) and in CI, and humans and
AI agents use the same interface.

**Status:** scaffold (wave 1). Nothing here does anything yet.

## Layout

| Path | Purpose |
| --- | --- |
| `crates/rt-core` | HTTP engine, variables, auth, runner |
| `crates/rt-format` | OpenCollection read/write, importers |
| `crates/rt-script` | Script sandbox (QuickJS) |
| `crates/rt-plugin` | Plugin host (WASM) and SDK |
| `crates/rt-cli` | CLI (`rt`), companion service, MCP server |
| `crates/rt-wasm` | The core as WebAssembly |
| `apps/ui` | Shared user interface |
| `apps/desktop` | Tauri desktop shell |
| `apps/web` | Web app |
| `plugins/odata` | OData V2/V4 plugin |

## Build

```sh
cargo build --workspace
cargo test --workspace
```

See `CLAUDE.md` for all checks CI runs.

## More

- [`examples/`](examples): example collections for S/4HANA (OData V2), CAP and RAP
- [`website/`](website): the project website (static, `/sap/`, `/download/`, `/docs/`)
- [`RELEASING.md`](RELEASING.md): signed release builds, secrets, verification
- [`plugins/README.md`](plugins/README.md): writing a plugin
- [`crates/rt-cli/README.md`](crates/rt-cli/README.md): the `rt` command line

## License

Apache License 2.0, see [LICENSE](LICENSE) and [NOTICE](NOTICE).
The RequestTailor name and logo are not covered by the Apache license.
