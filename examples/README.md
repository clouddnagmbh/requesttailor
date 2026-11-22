<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->

# Example collections

Collections to open in RequestTailor or run with `rt send`. Each folder is a complete
collection with its own README.

| Folder | Service | Shows |
| --- | --- | --- |
| [`s4hana-sales`](s4hana-sales) | OData V2, SAP Gateway style (S/4HANA) | Query options, `$expand`, `MERGE`, a function import, `$batch` with a changeset |
| [`cap-bookshop`](cap-bookshop) | CAP bookshop, OData V4 and the V2 adapter | Drafts (`draftEdit`, `draftActivate`), JSON and multipart `$batch` |
| [`rap-salesorder`](rap-salesorder) | RAP business object, OData V4 with draft | Edit, Prepare, Activate, Discard |

All three run against the synthetic OData test server (`cargo run -p rt-odata-testserver`,
port 47830) and use the OData plugin (choose **SAP and OData** at first start, or pass
`--plugin target/plugins/odata.wasm` to `rt send`). The test server and its data are written
for RequestTailor; no SAP software or SAP data is included.

`cargo test -p rt-format --test examples` checks that every collection here is valid
OpenCollection, reads without issues, round-trips byte for byte, and keeps secrets out of the
files.

SAP and S/4HANA are trademarks or registered trademarks of SAP SE in Germany and other
countries. RequestTailor is not affiliated with, endorsed or sponsored by SAP SE.
