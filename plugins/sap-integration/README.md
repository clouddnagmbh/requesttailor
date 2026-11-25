<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->

# SAP Integration Suite plugin

Test an iFlow of SAP Integration Suite (Cloud Integration) on the public plugin API. The core knows
nothing about it. Not bundled: install `target/plugins/sap-integration.wasm`
(`node scripts/build-plugins.mjs sap-integration`). **It asks for no rights**: no network, no
storage, no variables. The plugin sends nothing itself; every call is a request of the collection,
sent by the app with its own auth, send guard and masking.

## The flow

1. **Import the service key(s)** (`service-key` importer, JSON; one key per plan, up to two):
   a collection and an environment named after the tenant.
   - Folder `iflow` (runtime): OAuth 2.0 client credentials from `{{runtimeTokenUrl}}`,
     `{{runtimeClientId}}`, `{{runtimeClientSecret}}`. Request **Trigger iFlow**
     (`POST {{runtimeUrl}}/http/{{iflowPath}}`) with the assertions "status below 300" and "the
     runtime named the log", and an action that keeps the header `SAP_MessageProcessingLogID` in
     the runtime variable `mplId`.
   - Folder `message-processing-log` (API): OAuth from `{{tokenUrl}}`, `{{clientId}}`,
     `{{clientSecret}}`. **Log of the last trigger** (`GET .../MessageProcessingLogs('{{mplId}}')`)
     with the assertions `res.status eq 200` and `res.body.d.Status eq COMPLETED`;
     **Failed messages of the iFlow**; **Error text of the last trigger**.
   - The client secrets are **not** imported: `clientSecret` and `runtimeClientSecret` are secret
     variables without a value (the user sets them, they go to the key store). The report says so.
     A side without a key gets a URL derived from the other host when it follows the usual
     pattern (`it-cpi001` / `it-cpi001-rt`), marked as approximated.
2. **Run** the two requests (or `rt run`): the log assertion is green for `COMPLETED`, red for
   `FAILED` and red while the log still says `PROCESSING` (the log is written while the message
   runs: run it again). The checks are ordinary assertions and actions of the core, so the runner
   and CI need no plugin.
3. **Request tab "Integration"** on any request: keep the log id of its answer and create the
   log check with the status you expect (`FAILED` for a negative test).
4. **Response tab "Message log"** on a log answer: status, start, duration, ids; for failures a
   button that creates the request for the error text. The `response` hook adds the same in
   notes, and explains 401/403 (roles `ESBMessaging.send` for the endpoint, a monitoring role for
   the API).

Not done: duration and error text as assertions (see `API-WUENSCHE.md`), payload traces, suites
over several iFlows, deployed-artifact management.

Test data and the fake tenant: `testdata/SOURCES.md`, `tests/common/mod.rs` (ports 48818 to
48820).
