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

## Gateway errors and `sap-statistics` (wave 18, strand J)

`gateway/` holds synthetic golden files: error bodies in the three layouts (V2 JSON, V2 XML,
V4 JSON, plus a CAP error and an HTML page that is no OData error) and `sap-statistics`
header values with the segments the reader must make of them. Written by The RequestTailor
Authors; transaction IDs, codes, messages and the SAP Note number (`9999999`) are invented.

What is **not verified against a real system** (the reader is tolerant on purpose):

- **`sap-statistics` header.** Assumed: `key=number` pairs separated by `,` or `;`, in
  milliseconds unless the number carries a unit (`us`, `ms`, `s`), with the keys `total`,
  `fw`/`gw`/`hub`/`gateway` (gateway), `app`/`be`/`application` (application) and `db`/`database`
  (database). The real header's keys and unit must be checked against SAP's documentation and a
  system. Parts the reader does not understand are noted, never an error.
- **Error layouts.** From the public OData V2 and V4 specifications and the shape commonly
  seen from SAP Gateway and RAP (`innererror` with `transactionid`, `timestamp`,
  `Error_Resolution`, `errordetails` (V2) or `details` with `@Common.numericSeverity` (V4)).
- **Error log transactions** named in the hint: `/IWFND/ERROR_LOG` (gateway system) and
  `/IWBEP/ERROR_LOG` (backend). Unverified for V2 and V4; the hint is one sentence in
  `src/gateway/mod.rs` (`LOOKUP`).

The test server adds both to its S/4-style services (`testserver/src/gateway.rs`): the header
`sap-statistics: total=48,fw=7,app=21,db=9` on a successful answer to a request that sent
`sap-statistics: true` (or the query parameter), and, on every S/4 error, the full body with
transaction ID `0000000000000000000000000000RT01`, details and resolution (V2 as XML when only
`application/xml` is accepted).

`compensations/golden.json` (wave 19, strand H): synthetic responses of create, update and draft
requests in the shapes the OData V2 and V4 specifications describe (`Location`, `OData-EntityId`,
`d.__metadata`, `@odata.id`, `@odata.etag`, `ETag`); host names and ids are invented. The test
`tests/compensations.rs` derives the resource URL and the `If-Match` value from them with the
plugin's rules.
