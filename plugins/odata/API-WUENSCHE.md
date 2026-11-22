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
