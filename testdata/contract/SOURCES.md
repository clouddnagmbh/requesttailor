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

## Schemas (wave 18, strand H)

All written for RequestTailor tests (Apache-2.0, nothing copied from elsewhere).

- `graphql/shop-v{1,2,3}.graphql`: a synthetic shop schema. v2 only adds, rewords and loosens (nothing breaks, one
  deprecation); v3 breaks v1 in every way the rules of the contract name (removed type, field, argument, enum value,
  union member, interface and directive; changed types; stricter nullability; a new required argument and input field).
- `protobuf/v{1,2,3}/orders.proto` and `common/money.proto`: a synthetic Orders protocol with a message, nested types, a
  map, a `oneof`, enums and streaming rpcs. v2 only adds and changes wire-compatibly (`int32` to `int64`, a renamed
  field); v3 breaks v1 (changed and reused numbers, `repeated` to singular, incompatible type, removed field, enum
  value, rpc, service and messages, changed streaming, a moved `oneof` member).
- `schemas-collection/`: a collection with GraphQL and gRPC requests; its `opencollection.yml` names `api/shop.graphql`
  and `protos/orders.proto` (import path `protos`). The tests place a version of the schema files at those paths.
