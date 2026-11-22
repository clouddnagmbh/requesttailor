# rt-plugin-sdk

Rust SDK for RequestTailor plugins. A plugin is a single WebAssembly module
(`wasm32-unknown-unknown`) with its manifest embedded; the binary interface is
[`crates/rt-plugin/API.md`](../rt-plugin/API.md). The guide is
[plugins/README.md](../../plugins/README.md).

The SDK takes care of the binary interface, so a plugin is a trait implementation:

- **ABI glue** (wasm32 only, module `abi`): the exports `rt_alloc`, `rt_dealloc` and the body
  of `rt_call`; JSON in, JSON out; the `rt.host` import. All `unsafe` code of the SDK lives
  there.
- **`plugin!(MyPlugin, manifest = include_bytes!("../plugin.json"))`**: writes the manifest
  into the custom section `rt-plugin` and defines the `rt_call` export.
- **`Plugin` trait**: `prepare`, `response`, `render_tab`, `action`, `detect`, `import`. Hooks
  that a plugin does not implement answer with an error.
- **Host calls** as functions: `host::log`, `host::now`, `host::http`,
  `host::http_with_credentials`, `host::storage_get` / `storage_set` / `storage_delete`.
- **Helpers**: `set_header`, `info`, `warning`, `body_bytes`, `text_body`, `header`,
  `file_text`, `parse_manifest` (the host's manifest rules, for a test), `bail!`.
- **`testing`** (native only): `testing::call` runs a hook through the same JSON dispatch as
  `rt_call`; `MockHost` answers host calls with the permission checks of the real host.

```rust
use rt_plugin_sdk::api::{PluginContext, PluginRequest, PrepareReply};
use rt_plugin_sdk::{Plugin, Result, info, set_header};

pub struct Hello;

impl Plugin for Hello {
    fn prepare(&self, _: PluginRequest, _: PluginContext) -> Result<PrepareReply> {
        Ok(PrepareReply {
            patch: set_header("X-Hello", "world"),
            notes: vec![info("said hello")],
        })
    }
}

rt_plugin_sdk::plugin!(Hello, manifest = include_bytes!("../plugin.json"));
```

Build a plugin crate in `plugins/` with `node scripts/build-plugins.mjs` (output:
`target/plugins/<id>.wasm`). The crate also builds and tests natively; the ABI is compiled
only for wasm32. [`plugins/example`](../../plugins/example) uses every extension point.
