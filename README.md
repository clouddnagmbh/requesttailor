# RequestTailor

An API client that belongs to the developer: collections are plain files in your Git repo,
the same Rust core runs natively, in the browser (WebAssembly) and in CI, and humans and
AI agents use the same interface.

**Status:** pre-release. Working today: desktop app (macOS, Windows, Linux), web app and the
`rt` CLI on one core; collections as OpenCollection files with environments, secrets in the
OS keychain, Git without staging, importers with a loss report, history and search; OAuth 2.0,
AWS SigV4, client certificates, cookies; WebAssembly plugins with a public API, and the
optional OData plugin for SAP S/4HANA, CAP and RAP. Not yet: scripts and tests in requests,
the MCP server, GraphQL.

Try it with the demo collections: <https://github.com/clouddnagmbh/requesttailor-demos>.

## Layout

| Path | Purpose |
| --- | --- |
| `crates/rt-core` | HTTP engine, variables, auth, runner |
| `crates/rt-format` | OpenCollection read/write, importers |
| `crates/rt-script` | Script sandbox (QuickJS, planned) |
| `crates/rt-plugin` | Plugin API and host (WASM) |
| `crates/rt-plugin-sdk` | Rust SDK for plugins |
| `crates/rt-app` | Command layer of the apps |
| `crates/rt-git` | Git (gitoxide), native only |
| `crates/rt-cli` | CLI (`rt`), companion service (MCP server planned) |
| `crates/rt-wasm` | The core as WebAssembly |
| `apps/ui` | Shared user interface |
| `apps/desktop` | Tauri desktop shell |
| `apps/web` | Web app |
| `plugins/example` | Example plugin using every extension point |
| `plugins/odata` | OData V2/V4 plugin, its test server and synthetic test services |
| `sdk/typescript` | TypeScript types of the plugin API |

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
