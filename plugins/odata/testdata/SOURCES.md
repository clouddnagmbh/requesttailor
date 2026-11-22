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

Service roots served by the OData test server (`plugins/odata/testserver`):

| Root | File |
| --- | --- |
| `/sap/opu/odata/sap/ZRT_SALES_SRV/` | `s4/ZRT_SALES_SRV.edmx` |
| `/sap/opu/odata4/sap/zrt_salesorder/srvd_a2x/sap/zrt_salesorder/0001/` | `s4/zrt_salesorder_0001.edmx` |
| `/odata/v4/admin/` | `cap-bookshop/admin-v4.edmx` |
| `/odata/v2/admin/` | `cap-bookshop/admin-v2.edmx` |
