<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->

# Draft: SAP Community blog post

A draft for the launch post. Before posting: replace the placeholders in angle brackets,
check every fact against the published release, and pick the tags the community suggests
(for example ABAP RESTful Application Programming Model, SAP Cloud Application Programming
Model, SAP Gateway, SAP S/4HANA).

---

## Title

Testing OData services from S/4HANA, CAP and RAP with an open-source API client that keeps
requests in Git

## Post

Most of us test OData services the same way: open `$metadata` in one tab, a REST client in
another, copy an `X-CSRF-Token` by hand, forget the `If-Match` header, and keep a folder of
exported collections that nobody dares to merge. We built RequestTailor to make that part
boring.

RequestTailor is an API client under the Apache License 2.0. Three things set it apart:

- **Requests are plain files.** Every request is a small YAML file (OpenCollection, the format
  Bruno writes) in a folder you commit to Git. A change to a request is a readable diff in a
  pull request, next to the CDS model or the behaviour definition it tests.
- **No account.** Download it, open a folder, send. It works offline and does not talk to any
  server of ours. Passwords stay in your operating system's keychain; the files hold only the
  variable name.
- **The same core everywhere.** The desktop app (macOS, Windows, Linux), the browser version
  and the command line `rt` share one Rust core, so a request that works on your machine works
  the same way in a CI pipeline.

### What it does for OData

SAP support comes as an optional plugin. At first start the app asks whether you work with
SAP and OData; if you say no, you see none of it. If you say yes:

- **Entity browser** from `$metadata`, for V2 and V4: entity sets, keys, navigation,
  operations.
- **Query builder** for `$filter`, `$expand`, `$select`, `$orderby`, `$top`, `$skip` and
  `$count` / `$inlinecount`, written into the URL.
- **CSRF tokens** are fetched in the same session before every write and refreshed after a
  `403 Required`.
- **ETags** are remembered per entity; `If-Match` is set on updates and deletes.
- **`$batch`** with changesets, as multipart (V2 and V4) or JSON (V4), with the response
  shown part by part.
- **Drafts**: Edit, Prepare, Activate and Discard for RAP, `draftEdit`, `draftPrepare`,
  `draftActivate` and discard for CAP.
- **Landscapes**: DEV, QAS and PRD as environments with host, `sap-client` and language;
  `sap-client` and `sap-language` are added to every URL.
- **Importers** for `$metadata` files and API specifications downloaded from the SAP Business
  Accelerator Hub.

The plugin is written against the same public plugin API anyone can use; the core contains
no SAP-specific code.

### Try it in five minutes

The repository contains a synthetic OData test server and three example collections:
`s4hana-sales` (V2 in SAP Gateway style), `cap-bookshop` (CAP, V4 with drafts and the V2
adapter) and `rap-salesorder` (a RAP business object with the full draft cycle).

```sh
git clone https://github.com/clouddnagmbh/requesttailor
cd requesttailor
cargo run -p rt-odata-testserver      # http://127.0.0.1:47830
```

Open `examples/rap-salesorder` in the app, choose the environment Local, and run Edit,
Update draft, Prepare and Activate. Then point a copy at your own system.

In a pipeline:

```sh
rt send sales/read-sales-order.yml --env QAS --var password="$QAS_PASSWORD" --plugin odata.wasm
```

### Links

- Website and downloads: <website URL>
- Source code: https://github.com/clouddnagmbh/requesttailor
- OData guide: <website URL>/docs/odata/

It is a first release. Tell us which OData behaviour of your systems it gets wrong; issues and
pull requests are welcome.

SAP, S/4HANA, SAP Gateway and ABAP are trademarks or registered trademarks of SAP SE in Germany
and other countries. RequestTailor is not affiliated with, endorsed or sponsored by SAP SE.
