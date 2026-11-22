<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->

# Launch checklist

Steps that only the project owner can take. Everything else (website, docs, examples, release
pipeline, drafts in this folder) is prepared in the repository. Nothing here has been done
yet; tick each box when it is.

## Before the release

- [ ] **Apple**: Developer ID Application certificate and notarization access; add the
      `APPLE_*` secrets ([RELEASING.md](../RELEASING.md#apple-developer-id-and-notarization)).
- [ ] **Windows**: Azure Artifact Signing account, identity validation, certificate profile and
      app registration, then the `AZURE_*` secrets and variables; or a certificate with
      `WINDOWS_SIGNING=pfx` ([RELEASING.md](../RELEASING.md#windows-option-a-default-azure-artifact-signing)).
- [ ] **GPG release key**: create it, add `GPG_PRIVATE_KEY` and `GPG_PASSPHRASE`, keep the
      backup and revocation certificate offline, put the fingerprint into
      `website/download/index.html` and `RELEASING.md`.
- [ ] **Signed dry run**: Actions > Release > Run workflow (`signed` on). It must be green.
- [ ] **Version**: set `[workspace.package] version` in `Cargo.toml` (and `version` in
      `apps/desktop/src-tauri/tauri.conf.json`), review [RELEASE-NOTES.md](RELEASE-NOTES.md),
      commit with sign-off.
- [ ] **Hand demo against a real S/4HANA system** (wave 6 gate): with the OData plugin, read
      `$metadata`, browse, query, create and update with CSRF and ETag, a `$batch` with a
      changeset, one RAP draft cycle, DEV/QAS environments with `sap-client`. Note the system
      release and anything that behaved differently from the test server.
- [ ] **Legal notice**: if the website needs an imprint or legal notice for the publishing
      company, add it as a page under `website/` and link it in the footer of every page.

## Release

- [ ] Tag `v<version>` (signed tag) and push it. The workflow creates a **draft** release.
- [ ] Download every file from the draft on clean machines and check that the installers start
      without a warning ([RELEASING.md](../RELEASING.md#verifying-installers-start-without-a-warning)):
      macOS (`spctl`, `codesign`, `stapler`, double-click), Windows (`Get-AuthenticodeSignature`,
      `signtool verify`, SmartScreen), Linux (`gpg --verify`, `sha256sum -c`, AppImage, `.deb`).
      Record the results for the gate.
- [ ] Publish the draft release.

## Going public

- [ ] Make the repository `clouddnagmbh/requesttailor` public (confirmed once at the wave 6
      gate). Before that: `gitleaks git --no-banner --redact --config .gitleaks.toml .` on the
      full history, and check that `/docs/` (internal) never reached the history.
- [ ] Settings > Pages > Source: GitHub Actions; optional custom domain. Then Actions >
      **Website** > Run workflow. Open the published site and check `/`, `/sap/`,
      `/download/`, `/docs/` in light and dark mode and on a phone.
- [ ] Update the download page once the release is public, if file names or the fingerprint
      changed.
- [ ] Post the SAP Community article ([SAP-COMMUNITY-POST.md](SAP-COMMUNITY-POST.md)) after a
      last read for facts that changed. Check the community's rules for product announcements
      and the tags to use.
- [ ] Optional: announce elsewhere (project accounts, newsletters) with the same facts.
