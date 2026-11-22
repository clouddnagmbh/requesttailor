<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->

The first public release of RequestTailor, an open-source API client whose collections are
plain files in your Git repository. No account, no cloud, no telemetry.

## Highlights

- **Collections are files.** OpenCollection YAML, one file per request, environments as files.
  Bruno collections in OpenCollection YAML open unchanged; reading and writing gives
  byte-identical files.
- **Desktop, web and command line on one Rust core.** Native apps for macOS, Windows and
  Linux; the same interface in the browser through WebAssembly; `rt send` with a JSON report
  for pipelines. A conformance suite runs the same cases against the native and the
  WebAssembly core.
- **Secrets stay local.** OS keychain on the desktop, encrypted browser storage on the web,
  external secret managers (1Password, HashiCorp Vault, AWS Secrets Manager, Azure Key Vault,
  Google Secret Manager) through their own command lines, and a secret scanner that refuses
  commits with credentials (`rt scan` for hooks and pipelines).
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
| Windows x64 | `RequestTailor_<version>_windows_x64-setup.exe` (signed) |
| Linux x64 | `RequestTailor_<version>_linux_amd64.AppImage`, `RequestTailor_<version>_linux_amd64.deb` |
| CLI | `rt_<version>_macos_universal.zip`, `rt_<version>_windows_x64.zip`, `rt_<version>_linux_x64.tar.gz` |

Verify with `SHA256SUMS` and its signature `SHA256SUMS.asc` (key `release-key.asc`; compare
the fingerprint with the one on the website's download page).

## Not in this release

- The MCP server (planned).
- A hosted web app; the web app runs from the repository.
- Building plugins from TypeScript; plugins are written in Rust for now.
- Automatic updates: the app does not check for updates. Watch the repository's releases.

SAP, S/4HANA and SAP Gateway are trademarks or registered trademarks of SAP SE in Germany and
other countries. RequestTailor is not affiliated with, endorsed or sponsored by SAP SE.
