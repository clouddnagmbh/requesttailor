<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->

# Releasing

Releases are built by `.github/workflows/release.yml`. It never publishes anything: a tag push
ends in a **draft** release that a maintainer reviews and publishes by hand.

| Trigger | What happens |
| --- | --- |
| Push of a tag `v<version>` | Checks that the tag equals `[workspace.package] version` in `Cargo.toml` and that every signing secret is set, builds and signs everything, writes `SHA256SUMS` and `SHA256SUMS.asc`, creates a draft release with all files |
| Actions > Release > Run workflow | Dry run: the same build, kept as a workflow artifact (`release-dry-run-<version>`), no release. With `signed` off it needs no secrets at all |

What a release contains:

| File | Built on | Signed with |
| --- | --- | --- |
| `RequestTailor_<v>_macos_universal.dmg` | macOS (Apple silicon and Intel in one app) | Developer ID, notarized and stapled (app and disk image) |
| `RequestTailor_<v>_windows_x64-setup.exe` | Windows (NSIS installer) | Authenticode, timestamped (app and installer) |
| `RequestTailor_<v>_linux_amd64.AppImage`, `RequestTailor_<v>_linux_amd64.deb` | Ubuntu 22.04 (oldest supported glibc) | through `SHA256SUMS.asc` |
| `rt_<v>_macos_universal.zip` | macOS | Developer ID, notarized |
| `rt_<v>_windows_x64.zip` | Windows | Authenticode, timestamped |
| `rt_<v>_linux_x64.tar.gz` | Ubuntu 22.04 | through `SHA256SUMS.asc` |
| `SHA256SUMS`, `SHA256SUMS.asc`, `release-key.asc` | | GPG release key |

There is no updater. The app never checks for updates (product principle 2: no connection
the user did not ask for), so there is no update manifest, no updater key and no update
server. Users learn about releases from GitHub (watch, or the `releases.atom` feed).

Signing material is never committed. It lives only in GitHub secrets; the workflow writes it
to the runner's temporary folder, uses it, and deletes it at the end of the job.

## One-time setup

All secrets go to Settings > Secrets and variables > Actions > **Secrets**, the variables to
the **Variables** tab of the same page. A signed run checks them first and fails with a list
of what is missing.

### Secrets and variables

| Name | Kind | For | Value |
| --- | --- | --- | --- |
| `APPLE_CERTIFICATE` | secret | macOS | The Developer ID Application certificate with its private key, as `.p12`, base64-encoded |
| `APPLE_CERTIFICATE_PASSWORD` | secret | macOS | The password of that `.p12` |
| `APPLE_SIGNING_IDENTITY` | secret | macOS | The certificate's name, e.g. `Developer ID Application: Example GmbH (AB12CD34EF)` |
| `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | secrets | notarization, option 1 | Apple ID email, an app-specific password for it, the 10-character team ID |
| `APPLE_API_ISSUER`, `APPLE_API_KEY`, `APPLE_API_KEY_P8` | secrets | notarization, option 2 | App Store Connect API key: issuer ID, key ID, and the text of the `.p8` file |
| `WINDOWS_SIGNING` | variable | Windows | `azure` (default) or `pfx` |
| `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_TENANT_ID` | secrets | Windows, `azure` | The app registration that may sign |
| `AZURE_SIGNING_ENDPOINT` | variable | Windows, `azure` | The region endpoint of the signing account, e.g. `https://weu.codesigning.azure.net` |
| `AZURE_SIGNING_ACCOUNT`, `AZURE_CERTIFICATE_PROFILE` | variables | Windows, `azure` | Names of the signing account and the certificate profile |
| `WINDOWS_CERTIFICATE`, `WINDOWS_CERTIFICATE_PASSWORD` | secrets | Windows, `pfx` | The code signing certificate as `.pfx`, base64-encoded, and its password |
| `WINDOWS_TIMESTAMP_URL` | variable | Windows, `pfx` | RFC 3161 timestamp server of your CA (default `http://timestamp.digicert.com`) |
| `GPG_PRIVATE_KEY`, `GPG_PASSPHRASE` | secrets | checksums | The release signing key (ASCII-armored secret key) and its passphrase |

Base64 without line breaks: `base64 -i file.p12 | pbcopy` (macOS), `base64 -w0 file.pfx`
(Linux), `[Convert]::ToBase64String([IO.File]::ReadAllBytes("file.pfx")) | Set-Clipboard`
(PowerShell).

Optional hardening: move the secrets into a GitHub environment (for example `release`) that
only tags matching `v*` may use and that needs a reviewer, and add `environment: release` to
the three jobs.

### Apple: Developer ID and notarization

1. The organization needs a membership in the Apple Developer Program. Only the account
   holder can create Developer ID certificates.
2. Create a **Developer ID Application** certificate: in Xcode (Settings > Accounts > Manage
   Certificates > + > Developer ID Application), or at developer.apple.com > Certificates with
   a certificate signing request from Keychain Access.
3. In Keychain Access, export the certificate together with its private key as `.p12` with a
   strong password. Store it as `APPLE_CERTIFICATE` (base64) and `APPLE_CERTIFICATE_PASSWORD`.
   `security find-identity -v -p codesigning` prints the exact name for
   `APPLE_SIGNING_IDENTITY`.
4. Notarization, one of:
   - **Apple ID**: at account.apple.com > Sign-In and Security > App-Specific Passwords,
     create one for "RequestTailor CI". Store `APPLE_ID`, `APPLE_PASSWORD` (the app-specific
     password, not the account password) and `APPLE_TEAM_ID` (developer.apple.com >
     Membership).
   - **App Store Connect API key** (preferred: not tied to a person): App Store Connect >
     Users and Access > Integrations > Team Keys, create a key with the Developer role,
     download the `.p8` (only possible once). Store `APPLE_API_KEY` (key ID),
     `APPLE_API_ISSUER` (issuer ID) and `APPLE_API_KEY_P8` (the file's text, including the
     BEGIN and END lines). When these are set, they are used instead of the Apple ID.

The workflow imports the certificate into a temporary keychain, lets Tauri sign, notarize and
staple the app, signs and notarizes the disk image and `rt` itself, and deletes the keychain.
The app runs with the hardened runtime and needs no extra entitlements today.

### Windows, option A (default): Azure Artifact Signing

Azure Artifact Signing (formerly Trusted Signing) keeps the key in Microsoft's HSMs and issues
short-lived certificates; every signature is timestamped, so it stays valid.

1. In an Azure subscription, create an **Artifact Signing account** in a region near you and
   note its endpoint (for example `https://weu.codesigning.azure.net` for West Europe).
2. Complete **identity validation** for the organization (Public Trust). Microsoft checks
   the company's registration; this can take several days. Eligibility rules are on
   Microsoft's Artifact Signing pages.
3. Create a **certificate profile** of type Public Trust on the validated identity.
4. In Microsoft Entra ID, create an **app registration** for CI with a client secret. On the
   signing account (or the profile), give it the role **Artifact Signing Certificate Profile
   Signer** (shown as *Trusted Signing Certificate Profile Signer* in older portals).
5. Store `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_TENANT_ID` as secrets and
   `AZURE_SIGNING_ENDPOINT`, `AZURE_SIGNING_ACCOUNT`, `AZURE_CERTIFICATE_PROFILE` as
   variables. Leave `WINDOWS_SIGNING` unset or set it to `azure`.

The workflow installs `artifact-signing-cli` (pinned in `ARTIFACT_SIGNING_CLI_VERSION`, MIT
license, a CI tool that is not shipped) and gives it to Tauri as `bundle.windows.signCommand`;
Tauri calls it for every binary it signs, the app and the installer among them. `rt.exe` is signed the same
way. It uses the Azure CLI, .NET and `signtool`, which GitHub's Windows runners have.

### Windows, option B: a certificate file (`pfx`)

Set the variable `WINDOWS_SIGNING` to `pfx` and store the certificate as
`WINDOWS_CERTIFICATE` (base64 of the `.pfx`) and `WINDOWS_CERTIFICATE_PASSWORD`. The workflow
imports it into the runner's user certificate store and Tauri signs with `signtool` by
thumbprint (`certificateThumbprint`, SHA-256, `WINDOWS_TIMESTAMP_URL`).

Since June 2023, certificate authorities issue new code signing keys only on hardware tokens
or in cloud HSMs, so a `.pfx` with the key inside exists only for older certificates. With a
cloud HSM service (Azure Key Vault with AzureSignTool, DigiCert KeyLocker, SSL.com eSigner and
similar), replace the `signCommand` in the step "Release configuration for Tauri" and the
command in "Sign rt (Windows, ...)" with the vendor's signing tool; `%1` stands for the file.

### GPG release key

The key signs `SHA256SUMS`, which covers every file, including the Linux packages.

```sh
gpg --quick-gen-key "RequestTailor Release Signing <release@example.org>" ed25519 sign 3y
gpg --list-secret-keys --keyid-format long            # the fingerprint
gpg --armor --export-secret-keys <FINGERPRINT>        # -> secret GPG_PRIVATE_KEY
gpg --output revoke.asc --gen-revoke <FINGERPRINT>    # keep offline
```

Store the passphrase as `GPG_PASSPHRASE`. Keep a backup of the secret key and the revocation
certificate offline, not in the repository. Then publish the fingerprint where users compare
it: replace the placeholder on `website/download/index.html` ("Release key fingerprint") and
add it here:

Release key fingerprint: *to be added with the first release*

The workflow exports the public key as `release-key.asc` into every release.

### Website

Settings > Pages > Build and deployment > Source: **GitHub Actions**. Publishing is then
Actions > **Website** > Run workflow (`.github/workflows/website.yml`, manual only). For a
custom domain, set it in the same settings page.

## Making a release

1. Set the version in `Cargo.toml` (`[workspace.package] version`). The workflow passes it to
   Tauri, so `tauri.conf.json` may stay as it is, but keeping its `version` equal avoids
   confusion in local builds.
2. Update the release notes (`launch/RELEASE-NOTES.md` is used as the draft's text when it
   exists; otherwise GitHub generates notes).
3. Commit with sign-off (`git commit -s`) and wait for CI (`ci-ok`) to be green.
4. Optional: Actions > Release > Run workflow on that commit, as a signed dry run.
5. Tag and push: `git tag -s v0.1.0 -m "RequestTailor 0.1.0" && git push origin v0.1.0`.
6. When the workflow is green, open the draft under Releases, check the files, verify them as
   below on clean machines, then publish.

If a run fails after the draft was created, delete the draft (and the tag, if it has to move)
before running again.

## Verifying: installers start without a warning

The wave 6 gate asks for signed installers that start without a warning. CI checks the
signatures; whether the system shows a warning can only be seen on a real machine. Do this on
machines that have never seen the app, with files downloaded through a browser (so they carry
the quarantine or Mark-of-the-Web flag).

### macOS

```sh
xcrun stapler validate RequestTailor_<v>_macos_universal.dmg        # "The validate action worked!"
spctl --assess --type open --context context:primary-signature -vv RequestTailor_<v>_macos_universal.dmg
# open the disk image, drag the app to Applications, then:
codesign --verify --deep --strict --verbose=2 /Applications/RequestTailor.app
spctl --assess --type execute -vv /Applications/RequestTailor.app  # "accepted", "source=Notarized Developer ID"
xcrun stapler validate /Applications/RequestTailor.app
```

Then open the app by double-click. Expected: at most the standard one-time question "…is an
app downloaded from the Internet. Are you sure you want to open it?" with the developer's
name. Not acceptable: "cannot be opened because the developer cannot be verified" or "Apple
could not verify … is free of malware". For `rt`: unzip, run `./rt --version` from Terminal;
it must run without a Gatekeeper dialog (`spctl` does not assess command line tools; use
`codesign -dv --verbose=2 rt` to see the Developer ID and the timestamp).

### Windows

```powershell
Get-AuthenticodeSignature .\RequestTailor_<v>_windows_x64-setup.exe | Format-List Status, SignerCertificate, TimeStamperCertificate
signtool verify /pa /v .\RequestTailor_<v>_windows_x64-setup.exe     # from the Windows SDK
```

Then run the installer from the browser's download list. Expected: the User Account Control
prompt shows the verified publisher name, and Microsoft Defender SmartScreen does not show
"Windows protected your PC". SmartScreen decides by the reputation of the file and the
publisher, not by the signature alone: a new publisher or a new certificate can get that
screen for the first downloads, and since 2024 EV certificates no longer skip it. If it
appears, note it in the gate report; it goes away as the release is downloaded, and files can
be submitted for analysis to Microsoft (Microsoft Security Intelligence, "Submit a file", as a
software developer). Check `rt.exe` the same way with `Get-AuthenticodeSignature`.

### Linux and the checksums

```sh
gpg --import release-key.asc
gpg --verify SHA256SUMS.asc SHA256SUMS          # "Good signature", fingerprint as published
sha256sum -c SHA256SUMS --ignore-missing
chmod +x RequestTailor_<v>_linux_amd64.AppImage && ./RequestTailor_<v>_linux_amd64.AppImage
sudo apt install ./RequestTailor_<v>_linux_amd64.deb
```

## Not included (yet)

- Auto-update (deliberately, see above), Windows on Arm, Linux on Arm, RPM and MSI packages.
- Package managers (Homebrew, winget, Flatpak, apt repository).
- Build provenance attestations and an SBOM.
