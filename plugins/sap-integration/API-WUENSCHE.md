<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->

# Extension points the Integration Suite plugin is missing

The plugin uses only the public plugin API (`crates/rt-plugin/API.md`). Needs the API does not
meet are listed here instead of changing the core. Each entry: what, why, the workaround in use,
a proposal.

## Open

### Assertions the plugin computes

- **What:** a way for a plugin to contribute a check on a response, evaluated like an assertion
  (green or red in the report, in the runner and in `rt run`).
- **Why:** the log answer carries `LogStart` and `LogEnd` as `/Date(ms)/` strings. "The message
  took less than 5 s" is a difference of two such values, which the assertion expressions
  (`res.body...` paths and operators) cannot compute. The same goes for "the error text mentions
  X" when the text is a second request.
- **Workaround:** the status is an ordinary assertion (`res.body.d.Status eq COMPLETED`); the
  duration is shown in the Message log tab and in a note. A threshold on it is not enforceable.
- **Proposal (additive):** `Contributions.checks`: data rules like `paging` (a match on host or
  URL pattern plus a small expression), or a hook `check(request, response) -> [{ name, passed,
  detail }]` whose results the core adds to `SendReport.assertions`. Strand V (payload trace
  comparison) would use it as well.

### Waiting for an asynchronous result

- **What:** retry a request until an assertion holds, with a limit.
- **Why:** the log of a message can read `PROCESSING` or answer 404 for a moment. A test that
  triggers and checks in one run needs "check again in a second, at most five times".
- **Workaround:** the assertion is red with a hint, and the user runs the log request again
  (or the runner repeats it).
- **Proposal:** a request setting `retryUntil: { assertion, every_ms, max }` in the core, which
  also serves other asynchronous APIs, or a plugin-side `response` reply `retry_after_ms`.

### Variables derived while preparing

- **What:** derive the runtime URL of a tenant from its API URL (`it-cpi001` to
  `it-cpi001-rt`) at send time instead of at import time.
- **Why:** the importer writes a derived value into the environment where one of the two keys is
  missing and marks it approximated; a variable function would keep one source.
- **Workaround:** the derived value is a plain environment variable the user can correct.
- **Proposal:** none needed; listed so strand V does not rebuild it differently.

### Network from tabs for "open in the tenant"

The `AlternateWebLink` of a log entry (a link into the tenant's monitor) could be opened from the
tab. There is no API call that opens a URL; the link is shown as text only.
