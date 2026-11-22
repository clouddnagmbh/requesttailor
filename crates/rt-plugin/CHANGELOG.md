# Plugin API changelog

Every change to the plugin API (`API.md`, `src/api.rs`) is listed here, newest first.
Additive changes keep `API_VERSION`; breaking changes bump it.

## API version 1, additions (wave 6)

Additive; `API_VERSION` stays 1, plugins built for wave 5 run unchanged:
- Manifest: `contributes.welcome` (first-start choice of bundled plugins).
- Tabs: `place: "collection"` (panel of the open collection, `request_path` `""`).
- `render-tab` and `action` get `context` (environment, non-secret variables).
- `ViewReply`: `create_requests` and `add_environments` (unsaved additions to the
  collection; secret variables without values).
- `http` host call: `session: true` uses the cookie jar of the send.
- Rust SDK: `Tab::context`, `Action::context`, `host::http_in_session`.

## API version 1, clarifications (wave 5)

Documentation only, no ABI change:
- Buffer ownership: the host frees the `rt_call` input and reply with `rt_dealloc`; the
  request of `rt.host` stays the plugin's, and the plugin frees the reply the host
  allocated with `rt_alloc`.
- Each call runs in a fresh instance.
- `http`: credentials only for the request's origin, no redirects followed, 30 s timeout,
  bodies cut at 8 MiB. Storage limits: keys 1 to 256 bytes, values at most 1 MiB.
- `{"ok": null}` is a valid `prepare` and `response` reply.
- Updates that ask for more permissions are confirmed again.
- Where installed plugins live (`plugins/` in the data folder, `RT_DATA_DIR`).

## API version 1 (wave 5)

- First public version: manifest in the `rt-plugin` custom section, permissions (`network`,
  `storage`, `variables`), hooks `prepare`, `response`, `render-tab`, `action`, `detect`,
  `import`, host calls `log`, `now`, `http`, `storage-get`, `storage-set`,
  `storage-delete`, views with the blocks `heading`, `text`, `notice`, `key-value`,
  `table`, `code`, `tree`, `form`, `actions`.
- `API.md` links the developer guide `plugins/README.md` (Rust SDK `crates/rt-plugin-sdk`,
  TypeScript types `sdk/typescript`). Documentation only; the interface is unchanged.
