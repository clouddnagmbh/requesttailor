# RequestTailor plugin API, version 1

This document and `src/api.rs` define the plugin API. They are public and versioned: every
change needs an entry in `CHANGELOG.md`. Additive changes keep the version. These are new
optional fields, new hooks, new host calls and new view blocks. Anything else bumps it, and
the host keeps running plugins built for the older versions.

## What a plugin is

A plugin is a single WebAssembly module (`.wasm`), with no native code and no archive. It
carries its manifest as UTF-8 JSON in the custom section named `rt-plugin`. The type is
`api::Manifest`:

```json
{
  "id": "jwt-inspector",
  "name": "JWT inspector",
  "version": "1.0.0",
  "api": 1,
  "description": "Shows the claims of JWTs in responses.",
  "license": "Apache-2.0",
  "permissions": { "network": [], "storage": false, "variables": false },
  "contributes": {
    "prepare": false,
    "response": true,
    "tabs": [{ "id": "claims", "title": "JWT", "place": "response", "content_types": ["application/json"] }],
    "importers": []
  }
}
```

- `id`: `[a-z0-9][a-z0-9-]*`, at most 64 characters, unique per installation.
- `api`: the API version the plugin was built for. The host runs `1 <= api <= API_VERSION`.

## Extension points

| Hook | When | Gets | Returns |
| --- | --- | --- | --- |
| `prepare` | before every send, after auth, before signing | masked request, context | `RequestPatch`, notes |
| `response` | after every response | masked request, response, context | notes |
| `render-tab` / `action` | when the user opens a plugin tab or clicks in it | request spec, response, tab state | `View`, new state, optional request update |
| `detect` / `import` | import dialog | files | match yes/no; OpenCollection files plus a loss report |

The CLI runs `prepare` and `response` exactly like the apps. `rt send` shows the notes. Tabs
exist only in the apps. Importers also run in `rt import --plugin`.

Order: plugins run in installation order. Each `prepare` sees the patches of the plugins
before it. An `error` reply to `prepare` stops the send with that message (`SendError::Plugin`).
An `error` reply to `response` becomes an error note.

## Binary interface

The module **exports**:

| Export | Signature | Meaning |
| --- | --- | --- |
| `memory` | memory | linear memory, at most 64 MiB |
| `rt_alloc` | `(len: i32) -> i32` | allocate `len` bytes for the host to write into |
| `rt_dealloc` | `(ptr: i32, len: i32)` | free what `rt_alloc` or `rt_call` returned |
| `rt_call` | `(ptr: i32, len: i32) -> i64` | handle one `api::Call` (JSON at `ptr`); returns `(reply_ptr << 32) \| reply_len` of an `api::Reply` JSON |

The module **imports** from module `rt`:

| Import | Signature | Meaning |
| --- | --- | --- |
| `host` | `(ptr: i32, len: i32) -> i64` | one `api::HostCall` (JSON); returns `(ptr << 32) \| len` of a `Reply` JSON, allocated with the plugin's `rt_alloc` |

Buffer ownership:
- For `rt_call`, the host allocates the input buffer with `rt_alloc(len)` and writes the
  `Call` JSON into it. After `rt_call` returns, the host frees both the input buffer and the
  reply (`ptr`/`len` from the returned `i64`) with `rt_dealloc`.
- For `rt.host(ptr, len)`, the host reads the request but does not free it; it stays the
  plugin's. The host allocates the reply in plugin memory with `rt_alloc(reply_len)` and
  returns exactly that length. The plugin frees the reply itself.

Each call runs in a fresh instance of the module, so nothing in linear memory or globals
survives from one call to the next. Keep state in `storage` or in a tab's `state`.

Calls that wait for something, like `http`, look synchronous to the plugin. The host suspends
the module while the request runs and resumes it with the reply. This works the same
natively, on iOS and in the browser.

Plugins compiled from JavaScript or TypeScript may also import this subset of
`wasi_snapshot_preview1`:
- `fd_write` (only fds 1 and 2, written to the log);
- `clock_time_get`, `random_get`;
- `environ_get`, `environ_sizes_get`, `args_get`, `args_sizes_get` (all empty);
- `proc_exit`.

Every other import fails to link, so the plugin does not load.

## Host calls and permissions

| `op` | Permission | Reply |
| --- | --- | --- |
| `log` | none | `null` |
| `now` | none | Unix milliseconds |
| `http` | `network` must cover the URL's host | `PluginResponse` |
| `storage-get`, `storage-set`, `storage-delete` | `storage` | value or `null` |

`network` entries:
- an exact host;
- `*.example.com` for subdomains;
- `{request}` for the host of the request the current hook runs for;
- `*` for any host.

An empty list means no network. Without the permission, `http` replies
`{"error": "permission denied: network"}`, and nothing goes on the wire.

`http` with `credentials: true` applies the auth of the current request: header, token or
signature. The plugin never sees those values. This only works in the `prepare` and
`response` hooks, and only for URLs with the same origin (scheme, host, port) as the
request. Otherwise the call is refused with an `error` reply and nothing is sent.

`http` does not follow redirects; the plugin gets the 3xx response and decides itself. It
times out after 30 seconds. Request and response bodies are cut at 8 MiB, and the response
the plugin gets is masked like any other.

`storage` is per plugin: keys are 1 to 256 bytes, a value is at most 1 MiB of JSON.

A `prepare` or `response` reply of `{"ok": null}` means "nothing to change, no notes".

The user grants a plugin's permissions at install time. In API version 1 the grant is all
or nothing. An update (same `id`, new `version`) that asks for more than was granted needs
the user's confirmation again.

## Where installed plugins live

The desktop app and `rt` share one plugin folder, `plugins/` in the app's data folder:
- macOS: `~/Library/Application Support/dev.requesttailor.desktop/plugins`;
- Windows: `%APPDATA%\dev.requesttailor.desktop\plugins`;
- Linux: `$XDG_DATA_HOME/dev.requesttailor.desktop/plugins` (default `~/.local/share`).

`RT_DATA_DIR` replaces the data folder (for tests and CI). Inside: `<id>.wasm` per plugin,
`plugins.json` (order, granted permissions, enabled state) and `storage/<id>.json`. `rt send`
runs the enabled installed plugins; `--no-plugins` skips them, and `--plugin FILE` adds one
for this send with the permissions its manifest asks for. The web app keeps plugins in the
browser's IndexedDB (`requesttailor-plugins`).

## Secrets and sandbox

- Secret values never reach a plugin. Before a request goes to a plugin, every value of a
  secret variable, every OAuth 2.0 token and every resolved credential is replaced by `***`.
- A patch value that contains `***` is refused, because it would send the mask.
- Variables, through `context.variables`, need the `variables` permission and contain no
  secrets.
- Plugins have no file system, no clock beyond `now`, no threads and no network except
  `http`.
- Each call runs under a fuel limit and a 64 MiB memory limit. A plugin that exceeds them,
  traps or returns invalid JSON fails that call. The host stays up, and the user sees which
  plugin failed.

## Views

Tabs return a declarative `View`. The host renders it with its own components; plugin code
never runs in the UI. The blocks are:
- `heading`, `text` (plain, no markup), `notice`;
- `key-value`, `table`, `code`;
- `tree` (items can trigger actions);
- `form` (text, select and checkbox fields; submit triggers `action` with the values);
- `actions` (buttons).

`state` is opaque to the host. It is passed back with the next call for the same tab, so
plugins can stay stateless. `update_request` replaces the request in the editor as an unsaved
change, for example after a query builder action.

## JSON

All messages are snake_case JSON, exactly as `src/api.rs` serializes them. Both sides
ignore unknown fields.
