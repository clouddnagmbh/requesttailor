# RequestTailor plugin API, version 1

This document and `src/api.rs` define the plugin API. They are public and versioned: every
change needs an entry in `CHANGELOG.md`. Additive changes keep the version. These are new
optional fields, new hooks, new host calls and new view blocks. Anything else bumps it, and
the host keeps running plugins built for the older versions.

How to write, build and test a plugin with the Rust SDK (`crates/rt-plugin-sdk`):
[plugins/README.md](../../plugins/README.md).

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
- `contributes.welcome` (optional, wave 6): `{ "label", "description" }`. Only plugins that
  ship with the app (bundled) use it: at first start the app asks which of them to enable,
  and bundled plugins without the user's choice stay disabled. Installed plugins ignore it.
- Tab `place`: `request`, `response` or `collection` (wave 6). A collection tab is a panel of
  the open collection, not of a request: `target.request_path` is `""` and `spec` is an empty
  request.

## Extension points

| Hook | When | Gets | Returns |
| --- | --- | --- | --- |
| `prepare` | before every send, after auth, before signing | masked request, context | `RequestPatch`, notes |
| `response` | after every response | masked request, response, context | notes |
| `render-tab` / `action` | when the user opens a plugin tab or clicks in it | request spec, response, tab state, context | `View`, new state, optional request update, new requests, new environments |
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

`http` with `session: true` (wave 6; implies `credentials: true`, same rules) also runs in
the session of the send: cookies of the collection's cookie jar for the selected
environment go along, and `Set-Cookie` of the reply is stored there. That way a token the
plugin fetches (for example a CSRF token bound to a session cookie) is valid for the request
itself. Hosts without a cookie jar treat it like `credentials: true`. Cookie values the jar
keeps are masked in the `Set-Cookie` headers the plugin gets.

In `render-tab` and `action` (wave 6) no request is being sent, so `{request}` covers:
- in a request tab, the host of the tab's request URL, resolved with the selected
  environment as for sending (a URL with unresolved placeholders or one that needs a secret
  value covers no host);
- in a collection tab, the hosts of the selected environment's non-secret variables whose
  resolved values are absolute `http(s)` URLs (for example `baseUrl`).

`credentials` and `session` stay refused in tabs: there is no send whose auth or cookies they
could use.

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

Since wave 6 a reply may also add to the collection; the app shows what will be added and
the user saves it like any other change:
- `create_requests`: `{ "folder", "name", "spec" }`, new request files (`folder` relative to
  the collection root, `""` for the root; folders are created). Existing names get a suffix.
- `add_environments`: `{ "name", "variables": [{ "name", "value", "secret" }] }`. A `secret`
  variable is created without a value (the user sets it, it goes to the key store); a plugin
  can never write secret values. An existing environment of that name keeps its variables
  and gets only the missing ones.

`render-tab` and `action` carry a `context` (wave 6): the selected environment and, with the
`variables` permission, the non-secret variables in scope, as for `prepare`. Older hosts send
none.

## JSON

All messages are snake_case JSON, exactly as `src/api.rs` serializes them. Both sides
ignore unknown fields.
