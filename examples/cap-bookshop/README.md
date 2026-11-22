<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->

# Example: CAP bookshop (OData V4 and V2)

The `AdminService` of a bookshop as the SAP Cloud Application Programming Model (CAP) serves
it: OData V4 with draft-enabled `Books` in `v4/`, and the same service through the OData V2
adapter in `v2/`. It runs against `rt-odata-testserver`, a synthetic service written for
RequestTailor (no CAP code, no `@sap/cds`); the requests fit a `cds watch` bookshop of the
same shape.

| Request | What it shows |
| --- | --- |
| `v4/list-books.yml` | `$filter`, `$select`, `$expand` with nested `$select`, `$orderby`, `$count` |
| `v4/read-book.yml` | Read the active book (`ID`, `IsActiveEntity=true`) with author and genre |
| `v4/create-book-draft.yml` | `POST` on a draft-enabled set creates a new draft |
| `v4/edit-book.yml` | Bound action `draftEdit`: a draft of an active book |
| `v4/update-book-draft.yml` | `PATCH` the draft |
| `v4/activate-book.yml` | Bound action `draftActivate` |
| `v4/discard-book-draft.yml` | `DELETE` the draft (CAP's discard) |
| `v4/batch.yml` | JSON `$batch`: one read, two creates in one atomicity group |
| `v2/list-books.yml` | V2 query options, `$inlinecount` |
| `v2/read-book.yml` | Read by key with `$expand` |
| `v2/create-book.yml` | `POST` |
| `v2/update-book.yml` | `MERGE` with `If-Match` from the remembered ETag |
| `v2/batch.yml` | Multipart `$batch` with a changeset |

## Run it

1. Start the test server: `cargo run -p rt-odata-testserver` (port 47830).
2. Open this folder in RequestTailor and choose **SAP and OData** at first start, so the
   bundled OData plugin is enabled.
3. Pick the environment **Local**. The bookshop needs no login.
4. In `v4/`, run Edit, Update draft and Activate in this order (or Discard instead of
   Activate). To activate a book created with Create, set `bookId` to its ID (`901`) first,
   for example with `--var bookId=901` in the CLI.

```sh
rt send examples/cap-bookshop/v4/list-books.yml --env Local \
  --plugin target/plugins/odata.wasm --pretty
```

Each folder sets its own `service` root and `newBookId`, so V4 and V2 creates do not collide.

## What the OData plugin does here

- Remembers ETags (`@odata.etag` in V4, `__metadata.etag` in V2) and sets `If-Match` on
  updates that have none.
- Fetches a CSRF token before writes when the service asks for one (CAP does not by default).
- Its **Draft** tab offers `draftEdit`, `draftPrepare`, `draftActivate` and discard for every
  draft-enabled entity set; the **Batch** tab composes `$batch` bodies.

## Notes

- The JSON batch format (`v4/batch.yml`) is part of OData 4.01; CAP accepts it. Use the
  multipart form (as in `v2/batch.yml`) for services that only speak OData 4.0.
- The multipart body in `v2/batch.yml` has LF line endings. If a server insists on CRLF,
  compose the batch in the plugin's **Batch** tab.

SAP is a trademark or registered trademark of SAP SE in Germany and other countries.
RequestTailor is not affiliated with, endorsed or sponsored by SAP SE.
