<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->

# Test data of the OData plugin

Everything here is **synthetic**, written by The RequestTailor Authors under Apache-2.0. Nothing
is copied from SAP systems, the SAP Business Accelerator Hub or SAP software (no `@sap/cds`);
names, IDs and values are invented. The files follow publicly documented protocol behaviour
(OData V2 and V4 specifications, OASIS and SAP vocabularies, SAP Gateway conventions such as
`sap:` annotations, `sap-client`, CSRF token fetch).

| Path | What |
| --- | --- |
| `s4/ZRT_SALES_SRV.edmx` | OData V2 service in SAP Gateway style (sales orders, items, customers, function imports) |
| `s4/zrt_salesorder_0001.edmx` | OData V4 service in RAP style with draft (`Common.DraftRoot`) |
| `s4/data.json` | Entities of both S/4-style services (V2 sets end in `Set`) |
| `cap-bookshop/admin-v4.edmx` | OData V4 bookshop AdminService in CAP layout, draft-enabled `Books` |
| `cap-bookshop/admin-v2.edmx` | The same service as OData V2 (V2 adapter layout, no drafts) |
| `cap-bookshop/data.json` | Books, authors, genres |
| `hub/API_RT_PRODUCT_SRV.json` | Synthetic. OpenAPI 3.0 JSON in the shape of an SAP Business Accelerator Hub download of an OData V2 API: `x-sap-api-type`, `x-sap-shortText` and other `x-sap-*` keys, servers for the sandbox (`https://sandbox.api.sap.com/s4hanacloud/sap/opu/odata/sap/API_RT_PRODUCT_SRV`) and `https://{host}:{port}/…`, security schemes `APIKey` (header) and Basic, tags, `$ref` parameters, bodies with and without example, `$batch` |
| `hub/api_rt_salesorder_v4.json` | Synthetic. The same shape for an OData V4 API (`x-sap-api-type: ODATAV4`, `/sap/opu/odata4/…` sandbox server), no tags, `examples`, an OAuth 2.0 scheme |
| `hub/API_RT_PRODUCT_SRV.edmx` | Synthetic. OData V2 `$metadata` in SAP Gateway style whose `atom:link rel="self"` points to the Hub sandbox, as the `sap-hub` importer recognises an EDMX from the Hub |

The `hub/` files are written by us from the publicly documented shape of Hub
specifications; no file was downloaded from the Hub, and the API names (`API_RT_…`) are
invented. How closely they match real downloads is not verified, so the `sap-hub`
detection is a heuristic (see `src/landscape/hub.rs`).

Service roots served by the OData test server (`plugins/odata/testserver`):

| Root | File |
| --- | --- |
| `/sap/opu/odata/sap/ZRT_SALES_SRV/` | `s4/ZRT_SALES_SRV.edmx` |
| `/sap/opu/odata4/sap/zrt_salesorder/srvd_a2x/sap/zrt_salesorder/0001/` | `s4/zrt_salesorder_0001.edmx` |
| `/odata/v4/admin/` | `cap-bookshop/admin-v4.edmx` |
| `/odata/v2/admin/` | `cap-bookshop/admin-v2.edmx` |

The test server keeps the data of `data.json` in memory, per start, and answers in the style
of SAP Gateway (V2), RAP (V4) and CAP: CSRF tokens bound to a session cookie, weak ETags with
`If-Match`, `$batch` (multipart; JSON batch on V4), drafts. The S/4 services need Basic auth
`RT_USER` / `RT_PASS` and accept only `sap-client=100`. The full list of behaviours is the doc
comment of `testserver/src/lib.rs`.
