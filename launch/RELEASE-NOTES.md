<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->

The first public release of RequestTailor, an open-source API client whose collections are
plain files in your Git repository. No account, no cloud, no telemetry.

## Highlights

- **Collections are files.** OpenCollection YAML, one file per request, environments as files.
  Bruno collections in OpenCollection YAML open unchanged; reading and writing gives
  byte-identical files.
- **Desktop, web and command line on one Rust core.** A native app for macOS; the same
  interface in the browser through WebAssembly (on Windows and Linux too); `rt send` and
  `rt run` with JSON and JUnit reports for pipelines, a GitHub Action and a Docker image. A
  conformance suite runs the same cases against the native and the WebAssembly core.
- **Scripts and tests.** JavaScript before and after a request, tests with `expect`, and
  declarative assertions and "set variable" actions that need no script. One sandbox
  (QuickJS inside WebAssembly) on every surface; Bruno, Postman and Insomnia scripts run
  through shims, and the import report lists the few APIs that are not supported
  (`crates/rt-script/SHIM-COVERAGE.md`). Scripts only run in collections you trust, asked
  once per collection and device. `rt run` runs a folder with data files and `--bail`,
  reports as text, JSON and JUnit, and exits 4 when a test fails.
- **Secrets stay local.** OS keychain on the desktop, encrypted browser storage on the web,
  external secret managers (1Password, HashiCorp Vault, AWS Secrets Manager, Azure Key Vault,
  Google Secret Manager) through their own command lines, and a secret scanner that refuses
  commits with credentials (`rt scan` for hooks and pipelines). `rt send` and `rt run`
  read secret variables from the same keychain, so a request sends from the command
  line as it does in the app.
- **Authentication:** Basic, Bearer, API key, OAuth 2.0 (client credentials, password,
  authorization code with PKCE, device code, SAML 2.0 bearer), AWS Signature Version 4,
  client certificates.
- **Git in the app** without a staging area: commit, branches, pull, push, conflicts.
- **Imports** from Postman, Insomnia, OpenAPI, cURL, HAR and `.http` files, each with a loss
  report.
- **Plugins** as sandboxed WebAssembly modules with permissions you grant; the same file runs
  on every surface.
- **OData plugin** for SAP S/4HANA, CAP and RAP, optional at first start: entity browser and
  query builder from `$metadata`, CSRF token handling, ETags and `If-Match`, `$batch` with
  changesets, draft actions, DEV/QAS/PRD landscapes with `sap-client`. Example collections in
  `examples/`.
- **Performance budgets** checked on every build: cold start under 500 ms, idle memory under
  150 MB, smooth scrolling through a 100 MB response, a request sent in at most two keystrokes
  after start.

## Downloads

| System | File |
| --- | --- |
| macOS (Apple silicon and Intel) | `RequestTailor_<version>_macos_universal.dmg` (signed, notarized) |
| CLI, macOS | `rt_<version>_macos_universal.zip` (signed, notarized) |
| CLI, Linux | `rt_<version>_linux_x86_64.tar.gz`, `rt_<version>_linux_aarch64.tar.gz` (static binaries) |

Windows and Linux: use the web app or the CLI; there are no desktop installers for them.

Verify with `SHA256SUMS` and its signature `SHA256SUMS.asc` (key `release-key.asc`; compare
the fingerprint with the one on the website's download page).

## Not in this release

- Protocols beyond HTTP (gRPC, WebSocket, Server-Sent Events, GraphQL), the MCP server and
  MCP client, the optional AI assistant, `rt mock` and `rt docs`: they come with the next
  release, whose draft notes are in `launch/wave8/RELEASE-NOTES.md`.
- A hosted web app; the web app runs from the repository.
- Building plugins from TypeScript; plugins are written in Rust for now.
- Automatic updates: the app does not check for updates. Watch the repository's releases.

SAP, S/4HANA and SAP Gateway are trademarks or registered trademarks of SAP SE in Germany and
other countries. RequestTailor is not affiliated with, endorsed or sponsored by SAP SE.
