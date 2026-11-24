<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->

# Test data of the `btp-destinations` importer

Everything here is **synthetic**, written by The RequestTailor Authors under Apache-2.0. No file
was exported from a real SAP BTP subaccount; hosts end in `.invalid`, user names, client IDs and
numbers are invented, and every secret value is a marker (`RT-CANARY-…`) that must never appear
in an imported file or loss report.

**The export formats are assumptions, not verified against a real export.** They follow the
publicly documented shape of the BTP cockpit's "Export Destination" (flat JSON with `Name`,
`Type`, `URL`, `Authentication`, `ProxyType`, `User`, `Password`, `clientId`, `clientSecret`,
`tokenServiceURL`, `sap-client`) and of the Destination service's `destinationConfiguration`
object, and a Java `.properties` file with the same keys (`#` comments, `\:` and `\=`
escapes). Verify against a real export before relying on the detection.

| File | What |
| --- | --- |
| `basic.json` | One destination, `BasicAuthentication`, `sap-client` 100 |
| `oauth.json` | `OAuth2ClientCredentials` with token service user and password |
| `none.json` | `NoAuthentication` |
| `multi.json` | A JSON array of three destinations (two Basic, one OAuth) |
| `wrapped.json` | The Destination service shape (`destinationConfiguration`) |
| `s4.properties` | The `.properties` export of one Basic destination |
| `unsupported.json` | SAML bearer, user token exchange, principal propagation, client certificate, on-premise (Cloud Connector) and a non-HTTP destination |
