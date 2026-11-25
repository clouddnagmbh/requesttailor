<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->

# Extension points the SOAP plugin is missing

The SOAP plugin uses only the public plugin API (`crates/rt-plugin/API.md`). Needs the API
does not meet are listed here instead of changing the core. Each entry: what, why, the
workaround in use, a proposal.

## Open

### Renewing a WS-Security timestamp when the body holds a secret

- **What:** a `prepare` patch that changes the body of a request whose body contains a secret
  value (the UsernameToken password).
- **Why:** the plugin gets the body masked (`***`) and a patch value containing `***` is
  refused, so it cannot rewrite `wsu:Created`/`wsu:Expires` of a request that also carries
  `{{wsse_password}}`. Timestamp and UsernameToken together are the common case.
- **Workaround:** `prepare` renews the timestamp only when the body holds no secret; otherwise
  it warns, and the SOAP tab has "Refresh timestamp", which writes fresh times into the request
  (an unsaved change). The lifetime defaults to 300 seconds, so a long-lived request needs the
  button again.
- **Proposal (additive):** either dynamic variables the core resolves while preparing
  (`{{$isoTimestamp}}`, `{{$isoTimestamp+300s}}`), which would also serve other plugins and
  scripts; or `RequestPatch.body_edits: [{ find, replace }]` applied by the host to the
  unmasked body, so a plugin can change text around a secret without seeing it.

### PasswordDigest

- **What:** `wsse:PasswordDigest` needs `Base64(SHA1(nonce + created + password))`.
- **Why:** the plugin never sees the password, and the core offers no such computation.
- **Workaround:** clear-text passwords only (a note warns when the URL is `http` and not
  loopback).
- **Proposal:** a variable function the core resolves, e.g. `{{wsse:digest(nonce, created,
  secretName)}}`, kept out of plugins' sight.

### Network from tabs

A "load WSDL from URL" action in a tab would need the host of a URL the user types in, which
`{request}` does not cover in tabs (same wish as in `plugins/odata/API-WUENSCHE.md`). Until then
the WSDL is imported from files (the import dialog also takes a URL for built-in formats, not
for plugin importers).
