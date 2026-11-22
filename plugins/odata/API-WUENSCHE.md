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

### Resend after an expired CSRF token (strand C)

- **What:** a way for the `response` hook to ask the host to send the request once more,
  with a patch (e.g. a fresh `X-CSRF-Token`).
- **Why:** SAP sessions expire. The cached token is then refused with `403` and
  `X-CSRF-Token: Required`. A browser-based Fiori client fetches a new token and repeats
  the request without the user noticing.
- **Workaround:** the `response` hook drops the cached token and adds the warning "CSRF
  token expired, send again". The user sends again, and `prepare` fetches a new token.
- **Proposal:** an optional `retry: RequestPatch` in `NotesReply`. The host honours it at
  most once per send, only for the same request, and runs `prepare` again first. The report
  shows both exchanges.

### The last response in request tabs (strand C)

- **What:** request tabs (`place: "request"`) get `response: null`. The response view of
  the same request is a different tab.
- **Why:** the `batch` tab composes a `$batch` and should show the results of its parts
  (status per part and changeset) after the send, next to the composition.
- **Workaround:** the `response` hook stores the parsed parts under
  `batch-result:<request path>` in plugin storage. The tab reads them from there. The
  results are those of the last send of that request file, also from the CLI.
- **Proposal:** also pass the latest response of the request (the shell's response store)
  to request tabs, as `Tab.response` / `target.response_id`. Or allow a tab to declare
  `place: "both"`.

### Listing storage keys (strand C)

- **What:** `storage-list` with a key prefix, or an expiry per key.
- **Why:** ETags are remembered per entity URL (`etag:<url>`), so storage must be kept
  bounded.
- **Workaround:** an index per service root (`etag-index:<root>`) holds at most 500
  entries. When it is full, the oldest entries are deleted. That costs one extra read and
  write per response.
- **Proposal:** a `storage-list { prefix }` host call (keys only), or `storage-set` with
  `expires_ms`.
