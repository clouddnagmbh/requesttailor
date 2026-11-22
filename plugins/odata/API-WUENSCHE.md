<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->

# Extension points the OData plugin is missing

The OData plugin uses only the public plugin API (`crates/rt-plugin/API.md`). When it needs
something the API does not offer, the need is described here instead of changing the core.
Each entry: what, why, the workaround in use, and a proposal. Entries are reviewed when the
next API version is planned.

## Granted in wave 6 (API version 1, additive)

These were known before the strands started and are part of the wave 6 interface commit:

- **Collection tabs** (`place: "collection"`): the entity browser and the system landscape
  belong to the collection, not to one request.
- **Adding requests and environments** (`ViewReply.create_requests`,
  `ViewReply.add_environments`): "create requests from `$metadata`" and the DEV/QAS/PRD
  template. Secret variables are created without values.
- **Context in tabs** (`render-tab`/`action` get `context`): the query builder and the
  landscape need the selected environment and its variables.
- **Session-bound host requests** (`http` with `session: true`): SAP binds the CSRF token to
  the session cookie; a token fetched outside the send's cookie jar is rejected.
- **First-start choice** (`contributes.welcome`): "whoever doesn't pick SAP at first start
  sees no SAP".

## Open

### Network from collection tabs (strand A)

- **What:** the entity browser (collection tab `entities`) wants to fetch `<root>$metadata`
  for a service root the user types in.
- **Why:** the manifest asks only for `{request}`, the host of the request a hook runs for.
  Tabs run for no request: the host passes no request URL to `render-tab`/`action`
  (`PluginHost::tab` in `crates/rt-plugin/src/host/mod.rs`), so `{request}` covers no host
  and `http` answers `permission denied: network`, also in request tabs. Asking for `*`
  instead would let the plugin reach every host, which "SAP and OData" should not need.
- **Workaround:** the tab tries the fetch (so it works once a host allows it) and, when it
  is refused, explains the way that always works: send `GET <root>$metadata` as a request;
  the `response` hook caches every `$metadata` response. The tab offers to add that request
  to the collection (`create_requests`). Sending it also applies the collection's auth, which
  a tab fetch would not have.
- **Proposal (additive):** let `{request}` in tabs cover the hosts of the selected
  environment's URL-valued variables (e.g. `baseUrl`), or a new network entry
  `{environment}` for exactly that; with `credentials: true` the collection's auth. Either
  way the user granted it at install, and the plugin still cannot pick arbitrary hosts.

### Larger storage values (strand A)

- **What:** the metadata cache stores one JSON value per service (`metadata:<root>`).
- **Why:** a storage value is at most 1 MiB. The models of the test services are a few KiB;
  very large S/4HANA services (hundreds of entity sets) could exceed it.
- **Workaround:** none needed so far; if `store` fails, the `response` hook adds a warning
  note and the send is unaffected.
- **Proposal:** a larger limit for single values or a documented total quota, so a plugin
  can split a model over several keys predictably.

### Multi-select form field (query builder, strand B)

- **What:** a form field kind `multiselect` (value: the chosen options, e.g. comma-separated)
  and an optional per-field `help` text.
- **Why:** `$select` is a choice of properties. With only text, select and checkbox fields,
  the query builder shows one checkbox per property; S/4HANA entity types often have 50 to
  200 properties, which makes the form very long. A help text per field would put the
  filter examples next to the `$filter` field instead of in a table above the form.
- **Workaround:** one checkbox per property (`select.<Name>`) plus a text field for paths and
  `*`; the filter help is a `table` block before the form.
- **Proposal:** additive in API version 1: `FieldKind::Multiselect` (hosts without it show a
  text field, so the value format must be typeable) and `FormField.help: Option<String>`.

### Buttons that submit a form (query builder, strand B)

- **What:** a form with more than one submit button, or `actions` that carry the values of a
  form.
- **Why:** the filter builder has "Add a row" and "Remove a row". As plain `actions` they get
  no form input, so values typed but not yet submitted are lost when a row is added.
- **Workaround:** the rows keep what was last submitted ("Set $filter"); "Add a row" only
  changes the number of rows.
- **Proposal:** additive: `Block::Form.actions: Vec<ActionButton>` (extra submit buttons; the
  action id is the button's `id`, the input the form's values).
