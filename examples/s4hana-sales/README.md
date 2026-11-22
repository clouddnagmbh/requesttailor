<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->

# Example: S/4HANA sales orders (OData V2)

An OData V2 service in the style of SAP Gateway on S/4HANA: sales orders with items and
customers, a function import, and a `$batch` with a changeset. It runs against
`rt-odata-testserver`, a synthetic service written for RequestTailor (no SAP software, no SAP
data); the same requests work against a real Gateway service with the same shape.

| Request | What it shows |
| --- | --- |
| `list-sales-orders.yml` | `$select`, `$orderby`, `$top`, `$inlinecount` on `SalesOrderSet` |
| `read-sales-order.yml` | Read by key, `$expand` of items and customer; remembers the ETag |
| `create-sales-order.yml` | `POST` with the CSRF token fetched for you |
| `update-sales-order.yml` | `MERGE` with `If-Match` from the remembered ETag |
| `open-order-count.yml` | A function import (`GET`, parameter in the query) |
| `batch.yml` | `$batch` (multipart/mixed): one read, one changeset with a create |

## Run it

1. Start the test server (port 47830):

   ```sh
   cargo run -p rt-odata-testserver
   ```

2. Open this folder in RequestTailor (desktop or web). At first start, choose **SAP and
   OData** so the OData plugin is enabled; it is bundled with the app and off otherwise.
3. Pick the environment **Local**. It sets `sapClient` (100), `sapLanguage` (EN) and the user
   `RT_USER`. The password is a secret variable without a value in the file: enter the test
   server's password `RT_PASS` once in Environments; it is kept in the OS keychain (in the
   web app, in encrypted browser storage), never in the collection.
4. Send the requests in order. Read a sales order before you update it, so the plugin knows
   its ETag.

From the command line, with the plugin built (`node scripts/build-plugins.mjs odata`):

```sh
rt send examples/s4hana-sales/read-sales-order.yml --env Local \
  --var password=RT_PASS --plugin target/plugins/odata.wasm --pretty
```

## What the OData plugin does here

- Adds `sap-client=100` and `sap-language=EN` to every URL from the environment, unless the
  URL already has them.
- Before `POST`, `PUT`, `PATCH`, `MERGE` and `DELETE`: fetches an `X-CSRF-Token` in the same
  session (cookies) and sets it; a `403` with `X-CSRF-Token: Required` makes the next send
  fetch a new one.
- Remembers ETags (`ETag` header, `__metadata.etag`) per entity and sets `If-Match` on
  updates that have none. The test server answers `428` without it, like a service with
  optimistic locking.

Without the plugin, reads work; writes need the CSRF token and `If-Match` header set by hand.

## Notes

- `MERGE` is the OData V2 update method SAP Gateway expects; `PATCH` is accepted by newer
  Gateway releases too.
- The multipart body in `batch.yml` uses LF line endings, as every text file in a collection
  does. If a server insists on CRLF, compose the batch in the plugin's **Batch** tab.
- Against a real system: add DEV, QAS and PRD environments in the plugin's **SAP systems**
  tab (host, `sapClient`, `sapLanguage`, user; the password stays a secret without a value)
  and point `service` at your service root.

SAP, S/4HANA and SAP Gateway are trademarks or registered trademarks of SAP SE in Germany and
other countries. RequestTailor is not affiliated with, endorsed or sponsored by SAP SE.
