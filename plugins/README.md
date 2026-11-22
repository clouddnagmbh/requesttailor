# Writing a plugin

A RequestTailor plugin is a single WebAssembly module: no native code, no archive, the same
file on desktop, web, CLI and iOS. It can change requests before they are sent, comment on
responses, add tabs to the request editor and the response view, and add importers. The
binary interface and the rules are in [`crates/rt-plugin/API.md`](../crates/rt-plugin/API.md);
this guide shows how to build one.

Plugins are written in Rust with [`rt-plugin-sdk`](../crates/rt-plugin-sdk). TypeScript has
the types and a test host, but no build to WebAssembly yet (see [TypeScript](#typescript)).
[`plugins/example`](example) uses every extension point and is a good start.

## A plugin in Rust

A plugin crate is an ordinary library in `plugins/<name>/`, a member of the workspace:

```
plugins/hello/
  Cargo.toml
  plugin.json     the manifest
  src/lib.rs      the hooks
  tests/hooks.rs  native tests
```

`Cargo.toml`:

```toml
[package]
name = "rt-plugin-hello"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
publish.workspace = true

[dependencies]
rt-plugin-sdk = { path = "../../crates/rt-plugin-sdk" }

[lints]
workspace = true
```

`plugin.json` declares what the plugin is, what it contributes and which rights it asks for
(`api::Manifest`):

```json
{
  "id": "hello",
  "name": "Hello",
  "version": "0.1.0",
  "api": 1,
  "description": "Says hello in a header.",
  "license": "Apache-2.0",
  "permissions": { "network": [], "storage": false, "variables": false },
  "contributes": { "prepare": true, "response": false, "tabs": [], "importers": [] }
}
```

`src/lib.rs` implements the hooks it contributes and connects them with `plugin!`:

```rust
use rt_plugin_sdk::api::{PluginContext, PluginRequest, PrepareReply};
use rt_plugin_sdk::{Plugin, Result, info, set_header};

pub const MANIFEST: &[u8] = include_bytes!("../plugin.json");

pub struct Hello;

impl Plugin for Hello {
    fn prepare(&self, _: PluginRequest, _: PluginContext) -> Result<PrepareReply> {
        Ok(PrepareReply {
            patch: set_header("X-Hello", "world"),
            notes: vec![info("said hello")],
        })
    }
}

rt_plugin_sdk::plugin!(Hello, manifest = MANIFEST);
```

On wasm32, `plugin!` writes the manifest into the `rt-plugin` custom section and defines the
`rt_call` export; the SDK adds `rt_alloc` and `rt_dealloc`. Natively it adds nothing, so the
crate builds and tests like any other.

### Build

```sh
node scripts/build-plugins.mjs          # every crate in plugins/ with a plugin.json
node scripts/build-plugins.mjs hello    # just plugins/hello
```

The result is `target/plugins/<id>.wasm` (`--out DIR` for another folder). The script needs
only the pinned toolchain with the `wasm32-unknown-unknown` target. It builds the crate as a
staticlib (profile `plugin`: release, size-optimized, LTO), links it with the toolchain's
`rust-lld` exporting exactly `rt_alloc`, `rt_dealloc` and `rt_call`, and then checks the
module: only allowed imports, all exports, one manifest section equal to `plugin.json`.

Do not use `cargo build --crate-type cdylib` for plugins. A cdylib exports every
`#[no_mangle]` symbol of every dependency; through the API types this includes wasm-bindgen's
helpers from the core's browser transport, and with them imports (`__wbindgen_*`) that no
plugin host links.

## Hooks

| Manifest (`contributes`) | `Plugin` method | Runs | Returns |
| --- | --- | --- | --- |
| `prepare: true` | `prepare(request, context)` | before every send, after auth, before signing | `PrepareReply`: a `RequestPatch` and notes |
| `response: true` | `response(request, response, context)` | after every response | `NotesReply` |
| `tabs: [...]` | `render_tab(tab)`, `action(action)` | the user opens a tab or clicks in it | `ViewReply` |
| `importers: [...]` | `detect(importer, files)`, `import(importer, files)` | the import dialog, `rt import` | match yes/no; `ImportReply` |

- Return `Err` (or `bail!("...")`) to report a problem. For `prepare` it stops the send with
  that message; for `response` it becomes an error note; tabs and importers show it.
- `prepare` and `response` also run in the CLI (`rt send`), exactly like in the apps. Tabs
  exist only in the apps.
- Plugins run in installation order, and each `prepare` sees the patches before it.
- The plugin value is created for every call. Keep it stateless; state belongs in the tab
  state or in storage.
- A panic traps the module. The SDK logs the panic message through the host first; the host
  reports the plugin as failed and stays up.

Requests and responses reach a plugin **masked**: every secret value, OAuth 2.0 token and
credential is `***`. A patch value that contains `***` is refused, since it would send the
mask. Build new values from your own data, never by copying header values.

## Host calls and permissions

`rt_plugin_sdk::host` has one function per host call. They look synchronous; for `http` the
host suspends the plugin while the request runs.

| Function | Needs |
| --- | --- |
| `host::log(level, message)` | nothing |
| `host::now()` (Unix ms) | nothing |
| `host::http(request)` | `permissions.network` covering the URL's host |
| `host::http_with_credentials(request)` | the same; applies the current request's auth (only in `prepare`/`response`; the plugin never sees the values) |
| `host::storage_get/set/delete(key, ...)` | `permissions.storage` (private per plugin and device) |

`permissions.network` entries: an exact host, `*.example.com` for subdomains, `{request}` for
the host of the request the hook runs for, `*` for any. Empty means no network at all, and
`http` fails with `permission denied: network`. `permissions.variables` fills
`context.variables` with the non-secret variables in scope.

Ask for as little as possible. The user sees the permissions before installing and grants
them all or nothing (API version 1). The example plugin needs none.

Plugins have no file system, no threads, no clock besides `now` and no network besides
`http`. Every call runs under a fuel limit and a 64 MiB memory limit.

## Views

Tabs return a declarative `View`; the host renders it with its own components, and plugin
code never runs in the UI. Blocks (`api::Block`): `Heading`, `Text` (plain, no markup),
`Notice`, `KeyValue`, `Table`, `Code`, `Tree` (items can trigger actions), `Form` (text,
select, checkbox; submit triggers `action` with the values in `Action::input`), `Actions`
(buttons).

- `ViewReply::state` is opaque to the host and comes back as `Tab::state` / `Action::state`
  with the next call for the same tab. The example keeps "statistics shown" there.
- `ViewReply::update_request` replaces the request in the editor as an unsaved change
  (request tabs only), for example after a query builder's "Apply".
- `content_types` in the manifest limits a response tab to matching `Content-Type`s.

## Importers

`detect` gets the files the user picked (`PluginFile`: name and base64 content; use
`file_text`/`file_bytes`) and says whether they look like its format. `import` converts them
into OpenCollection files (`[path, text]`, with an `opencollection.yml`) and a `LossReport`
(`format: ImportFormat::Plugin`) that lists everything it could not carry over. The host reads
the files back with `rt-format` before it writes anything; test that yourself with
`rt_format::read_collection` (see `plugins/example/tests/hooks.rs`).

## Testing

Natively, with the same JSON dispatch the `rt_call` export runs:

```rust
use rt_plugin_sdk::api::{Call, PrepareReply};
use rt_plugin_sdk::testing::{self, MockHost};

#[test]
fn prepare_sets_the_header() {
    let _host = MockHost::new().now(1_790_000_000_000).install();
    let reply: PrepareReply = testing::call(
        &Hello,
        &Call::Prepare {
            request: testing::request("GET", "https://api.example.com/"),
            context: testing::context("a.yml"),
        },
    )
    .unwrap();
    assert_eq!(reply.patch.set_headers[0].0, "X-Hello");
}

#[test]
fn manifest_is_valid() {
    rt_plugin_sdk::parse_manifest(MANIFEST).unwrap();
}
```

`MockHost` answers `log`, `now`, `storage-*` and `http` (with `on_http`) and checks
permissions like the real host (`granted(...)`, `request_host(...)`); `logs()`, `storage()`
and `calls()` show what the plugin did.

The built module can be tested too, with the test host of the TypeScript SDK on Node's
WebAssembly (`sdk/typescript/src/testing.ts`); `npm test -w sdk/typescript` does this for the
example plugin:

```ts
import { TestPlugin } from "../src/testing.ts";
const plugin = await TestPlugin.load(readFileSync("target/plugins/example.wasm"));
const reply = plugin.call({ hook: "prepare", request, context });
```

### Trying a plugin in the apps

- **CLI**: `rt send <request.yml> --plugin target/plugins/<id>.wasm` runs the plugin's
  `prepare` and `response` hooks; the notes are in the report. For example, with the test
  server: `cargo run -p rt-testserver -- --port 47830` and
  `cargo run -p rt-cli -- send testdata/conformance/collection/basics/get.yml --env local --var port=47830 --plugin target/plugins/example.wasm --pretty`.
- **Desktop** (`npm run dev -w apps/desktop`) and **Web** (`npm run dev -w apps/web`): install
  the `.wasm` in the plugin settings (the dialog shows the requested permissions), send a
  request, and look at the notes and the plugin's tabs; importers appear in the import
  dialog. Desktop and CLI share one plugin directory; the web app keeps plugins in browser
  storage.

## TypeScript

`sdk/typescript` has the types of every message (`src/api.ts`, checked against the Rust JSON)
and the test host (`src/testing.ts`). It does **not** yet have a build from TypeScript to a
plugin module. The obvious tool, Javy (QuickJS compiled to WebAssembly), does not fit the
binary interface of API version 1 as it is:

- Its modules import `fd_read`, `fd_seek`, `fd_close` and `fd_fdstat_get` from
  `wasi_snapshot_preview1`, which are outside the allowed WASI subset, so a host does not
  load them.
- They export `_start` and functions without parameters or results. A plugin needs
  `rt_alloc`, `rt_dealloc` and `rt_call(ptr, len) -> i64`, and has to import `rt.host`.
  Javy's input and output go through stdin and stdout instead.
- Bridging that needs a custom JavaScript runtime module: a Javy plugin or a QuickJS build of
  our own. Both compile QuickJS (C) for WebAssembly, which needs the WASI SDK in development
  and CI. A pure Rust engine would avoid that, at the cost of a second JavaScript engine and
  a much larger module.

Which way to go is open. Until then, write plugins in Rust, and use the TypeScript types for
tooling around plugins.

## Versioning

- `api` in the manifest is the plugin API version the plugin was built for. A host runs
  plugins with `1 <= api <= API_VERSION` (`rt_plugin_sdk::API_VERSION`).
- The API changes additively within a version (new optional fields, hooks, host calls,
  blocks); both sides ignore unknown fields. Anything else bumps the version, and hosts keep
  running plugins built for older versions. Every change is listed in
  [`crates/rt-plugin/CHANGELOG.md`](../crates/rt-plugin/CHANGELOG.md).
- A call for a hook the SDK does not know yet is answered with `unsupported hook: <name>`.
- `version` is the plugin's own semantic version. The id stays the same across versions;
  installing a newer version replaces the old one.

## Binary details

What the SDK does for you, for those who write a plugin without it:

- Buffers: the host writes each call into a buffer from `rt_alloc(len)`, calls
  `rt_call(ptr, len)`, reads the reply at `(result >> 32, result & 0xffffffff)` and frees
  both with `rt_dealloc`. For `rt.host(ptr, len)` the plugin owns its request buffer; the host
  allocates the reply with the plugin's `rt_alloc(reply_len)` and returns pointer and
  length, and the plugin frees it.
- Replies are `{"ok": ...}` or `{"error": "..."}` (`api::Reply`).
- The modules use the WebAssembly features the pinned Rust toolchain enables for
  `wasm32-unknown-unknown` by default (such as bulk memory, sign extension and reference
  types); the SDK enables nothing beyond them (no threads, no SIMD).
