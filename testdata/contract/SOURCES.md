# Contract fixtures

All files here are written for RequestTailor tests (Apache-2.0, nothing copied from elsewhere).

- `specs/v1.yaml`, `v2.yaml`, `v3.yaml`: three versions of a synthetic "Orders API". v1 is OpenAPI 3.0.3. v2 only adds
  (a new operation, an optional parameter, new fields, a new response) and rewords. v3 is OpenAPI 3.1.0 and breaks: a
  removed operation, a new required parameter, a removed parameter, a changed type, a removed field, a new required
  body field, a replaced success status. `nullable: true` (v1) became `type: [string, "null"]` (v3) on purpose: that is
  no change.
- `specs/mini.json`: a minimal JSON spec.
- `collection/`: a collection with one request per operation of v1; its `opencollection.yml` names the spec in
  `extensions.requesttailor.contract.spec` (`/specs/v1.yaml`, relative to the repository root (`testdata/contract` here)).
