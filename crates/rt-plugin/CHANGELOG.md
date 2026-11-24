# Plugin API changelog

Every change to the plugin API (`API.md`, `src/api.rs`) is listed here, newest first.
Additive changes keep `API_VERSION`; breaking changes bump it.

## API version 1, additions (wave 19)

Additive; `API_VERSION` stays 1, plugins built earlier run unchanged:
- `contributes.compensations`: a list of `{ "id", "match": { "hosts", "content_types",
  "url_pattern", "methods" }, "rule": { "id_from", "resource_url", "etag_from", "token",
  "not_reversible" } }`. Data only, no call per response. It tells the test-data journal how
  writing requests to matching hosts are undone: where the created resource's id is
  (`header:<Name>` or a JSONPath), how one resource's URL is made (`{collection}`, `{id}`),
  where the version for `If-Match` is, a token to fetch before writing, and entries that
  cannot be reverted with a reason. The app executes the undoing under its own rules
  (guard, production question, conflict check, audit); the plugin cannot send anything
  through this.

## API version 1, additions (wave 18)

Additive; `API_VERSION` stays 1, plugins built earlier run unchanged:
- `NotesReply.segments` (reply of the `response` hook): a list of
  `{ "id", "label", "ms", "parent" }` (`parent`: `"server"` or absent). They are parts of the
  time the server says it spent. The app shows them in their own line under the timing bar,
  as "reported by the server", never as part of the client's own phases.
- `contributes.paging`: a list of `{ "id", "match": { "hosts", "content_types",
  "url_pattern" }, "rule": { "rule": { "kind": "json-url" | "link-header" | "cursor", … },
  "merge": "<JSONPath>" } }`. Data only, no call per response; the rule is offered for
  matching responses. A rule of the collection or of the request wins.

## API version 1, clarifications (wave 17)

Host behavior only, no API change; `API_VERSION` stays 1:
- The app shows a "Copy" button on every `code` block of a plugin view; it copies `text`.
  Plugins need to do nothing; a view that wants a snippet copied just renders it as `code`.

## API version 1, additions (wave 8)

Additive; `API_VERSION` stays 1:
- The request model has `protocol` (`{"type": "grpc", …}` or `{"type": "websocket", …}`;
  absent for HTTP) and a GraphQL body (`{"type": "graphql", "query", "variables"}`).
  Plugins may leave `protocol` out (HTTP). Hooks (`before_send`, `after_response`) run for
  HTTP sends only, GraphQL included; sessions (WebSocket, SSE, gRPC) do not call them yet.

## API version 1, additions (wave 7)

Additive; `API_VERSION` stays 1:
- The request model (`spec` in `import` replies, `update_request`, `create_requests`) has
  `scripts`, `assertions` and `actions` (scripts, declarative assertions, "set variable"
  actions of the request). Plugins may leave them out (empty); the host always sends them.
  Plugins still never see secret values: files hold none, and scripts' output is not
  passed to plugins.

## API version 1, additions (wave 6)

Additive; `API_VERSION` stays 1, plugins built for wave 5 run unchanged:
- Manifest: `contributes.welcome` (first-start choice of bundled plugins).
- Tabs: `place: "collection"` (panel of the open collection, `request_path` `""`).
- `render-tab` and `action` get `context` (environment, non-secret variables).
- `ViewReply`: `create_requests` and `add_environments` (unsaved additions to the
  collection; secret variables without values).
- `http` host call: `session: true` uses the cookie jar of the send (implies
  `credentials: true`; cookie values the jar keeps are masked for the plugin).
- `{request}` in tabs: the tab's request URL (request tabs) or the selected environment's
  URL variables (collection tabs), resolved; `credentials`/`session` stay refused in tabs.
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
