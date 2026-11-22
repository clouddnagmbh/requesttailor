# plugins/example

An example plugin for the [plugin API](../../crates/rt-plugin/API.md). It uses every extension
point and asks for no permissions:

| Extension point | What it does |
| --- | --- |
| `prepare` | Sets an `X-Request-ID` header (`rt-<time>-<hash>`) unless the request has one, and adds a note with it. |
| `response` | Adds a note with status, size and content type, e.g. `200 · 2.0 KB · application/json`. |
| Response tab `summary` | Key-value block (status, type, size, final URL), a table of the response headers, and an action that adds body statistics (lines, words, JSON shape). Whether they are shown lives in the tab state. |
| Importer `url-list` | Text files (`.urls`, `.txt`) with one request per line, `[METHOD] URL`; `#` comments. Other lines show up in the loss report. Try [`examples/users.urls`](examples/users.urls). |

The manifest is [`plugin.json`](plugin.json); `rt_plugin_sdk::plugin!` embeds it into the
module's `rt-plugin` custom section.

```sh
node scripts/build-plugins.mjs example           # → target/plugins/example.wasm
cargo test -p rt-plugin-example                  # the hooks, natively
npm test -w sdk/typescript                       # the built .wasm on Node's WebAssembly
```

How to write your own: [plugins/README.md](../README.md).
