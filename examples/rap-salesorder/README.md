<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->

# Example: RAP sales order with draft (OData V4)

An OData V4 service in the style of an S/4HANA RAP business object (ABAP RESTful Application
Programming Model) with draft: the whole draft lifecycle from Edit to Activate, and a `$batch`.
It runs against `rt-odata-testserver`, a synthetic service written for RequestTailor (no SAP
software, no SAP data).

| Request | What it shows |
| --- | --- |
| `list-sales-orders.yml` | `$filter` on `IsActiveEntity`, `$select`, `$orderby`, `$count` |
| `read-sales-order.yml` | Read the active instance by its key (`SalesOrderUUID`, `IsActiveEntity=true`), `$expand=_Item` |
| `create-sales-order-draft.yml` | `POST` on a draft root creates a new draft (`IsActiveEntity=false`) |
| `edit-sales-order.yml` | Bound action `Edit`: a draft of an active instance |
| `update-sales-order-draft.yml` | `PATCH` the draft |
| `prepare-sales-order.yml` | Bound action `Prepare`: run the draft's validations |
| `activate-sales-order.yml` | Bound action `Activate`: the draft becomes the active instance |
| `discard-sales-order-draft.yml` | Bound action `Discard`: throw the draft away instead |
| `batch.yml` | `$batch` (multipart/mixed): one read, one changeset with a create |

## Run it

1. Start the test server: `cargo run -p rt-odata-testserver` (port 47830).
2. Open this folder in RequestTailor and choose **SAP and OData** at first start, so the
   bundled OData plugin is enabled.
3. Pick the environment **Local** (`sapClient` 100, `sapLanguage` EN, user `RT_USER`) and
   enter the test server's password `RT_PASS` for the secret variable `password` once. It is
   kept in the OS keychain, never in the collection.
4. Run Edit, Update draft, Prepare and Activate in this order. Discard instead of Activate
   throws the draft away. The plugin's **Draft** tab offers the same actions for any
   draft-enabled entity set.

The collection variable `salesOrderUUID` names the first sales order of the test data. To
continue with a draft you created, put the `SalesOrderUUID` from its response there.

```sh
rt send examples/rap-salesorder/edit-sales-order.yml --env Local \
  --var password=RT_PASS --plugin target/plugins/odata.wasm --pretty
```

## What the OData plugin does here

- `sap-client` and `sap-language` from the environment on every URL.
- The CSRF token for every modifying request, fetched in the same session.
- `If-Match` from the remembered ETag (`@odata.etag`) on `PATCH` and `DELETE`.

## Notes

- Bound actions are called with the schema namespace (`com.example.zrt_salesorder.Edit`);
  a RAP service of your own uses its namespace or the alias `SAP__self`.
- The multipart body in `batch.yml` has LF line endings. If a server insists on CRLF, compose
  the batch in the plugin's **Batch** tab.

SAP and S/4HANA are trademarks or registered trademarks of SAP SE in Germany and other
countries. RequestTailor is not affiliated with, endorsed or sponsored by SAP SE.
