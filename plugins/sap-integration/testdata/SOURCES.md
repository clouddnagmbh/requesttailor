<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->

# Test data of the SAP Integration Suite plugin

Everything here is synthetic and written for this repository (Apache-2.0). Nothing is copied from
SAP documentation, the SAP Business Accelerator Hub or a real tenant. Host names are made up
(`rt-test`, `rt-uaa`) and are never contacted: the tests point the variables at the fake tenant on
loopback (`tests/common/mod.rs`, ports 48818 to 48820).

| File | What it is |
| --- | --- |
| `service-key-api.json` | A service key shaped like the `oauth` object of the Cloud Integration API plan (`clientid`, `clientsecret`, `url`, `tokenurl`). The secret is a canary (`RT-CANARY-...`): no output of the plugin may contain it. |
| `service-key-runtime.json` | The same shape for the runtime (integration flow) plan: the host ends in `-rt`. |
| `service-key-uaa.json` | The generic BTP shape (`uaa` object plus a top-level `url`). |
| `not-a-key.json` | A JSON file that is not a service key (the importer must not claim it). |

The shapes follow what the public BTP documentation describes in prose (field names only); the
values are invented.

The fake tenant answers like the documented interfaces in the parts the tests need: an OAuth token
endpoint (client credentials), an iFlow endpoint (`POST /http/<path>`, header
`SAP_MessageProcessingLogID`) and the message processing log API
(`/api/v1/MessageProcessingLogs('<id>')`, `.../ErrorInformation/$value`, a filtered list; OData
V2 JSON with `/Date(ms)/` timestamps). It is written for this repository and holds no SAP content.
