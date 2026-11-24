<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->

# OData plugin

OData V2 and V4 for RequestTailor, as found in SAP S/4HANA, SAP CAP and RAP, built only on
the public plugin API (`crates/rt-plugin/API.md`). The RequestTailor core knows nothing about
OData or SAP.

- Entity browser from `$metadata` (V2 and V4), cached per service root
- Query builder for `$filter`, `$expand`, `$select`, `$orderby`, `$top`, `$skip`, `$count`
- CSRF token handshake, ETag and `If-Match`, `$batch` with changesets, draft actions
- SAP system landscapes (DEV/QAS/PRD) with `sap-client` and `sap-language`
- Copy as ABAP: the request tab "ABAP" turns the request into a snippet for ABAP Platform
  (`cl_http_client` through an RFC destination) or ABAP Cloud (`cl_web_http_client_manager`
  through a communication arrangement), with `/ui2/cl_json` for the response. Reads, create,
  update, delete and read-only `$batch`; the CSRF token is fetched when the code runs, `If-Match`
  comes from a variable, and logon data, tokens and cookies are never copied (placeholders in
  angle brackets instead). The snippets are checked against golden files
  (`testdata/abap/`), not against a system: review them before use.
- Importers for `$metadata` files and SAP Business Accelerator Hub API specifications
- Importer `btp-destinations`: exported SAP BTP destinations (JSON, one or several, or `.properties`)
  become environments with `baseUrl`, `sapClient` and, for Basic and OAuth client credentials,
  `user`/`password` or `tokenUrl`/`clientId`/`clientSecret`. Secrets are variables without a
  value, never read from the export; other authentication types, on-premise destinations and
  non-HTTP types keep only `baseUrl` and `sapClient` and appear in the loss report (property
  names, never values). The export formats are assumptions (`testdata/destinations/SOURCES.md`).

The app ships it bundled; at first start it is enabled only if you choose "SAP and OData".

Build: `node scripts/build-plugins.mjs odata` → `target/plugins/odata.wasm`.
Test server: `cargo run -p rt-odata-testserver` (port 47830), services in
`testdata/SOURCES.md`. Missing extension points: `API-WUENSCHE.md`.
