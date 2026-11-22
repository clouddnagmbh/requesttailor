# gRPC test data

Synthetic files written for RequestTailor's tests, Apache-2.0 like the rest of the
repository. Nothing here is copied from another project.

| File | Purpose |
| --- | --- |
| `protos/echo.proto` | Echo service of `rt_testserver::grpc` (unary, server, client and bidi streaming, an error status, metadata echo); imports `rt/common.proto` and the well-known `google/protobuf/timestamp.proto` |
| `include/rt/common.proto` | Shared types, found through the import path `include` |
| `collection/` | OpenCollection with gRPC requests against the test server (`config.protobuf` and per-request `protoFilePath`) |

The reflection messages the test server and the client use live in
`crates/rt-grpc/proto/reflection.proto` (field numbers of the public gRPC reflection protocol,
written for RequestTailor).
