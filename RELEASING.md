<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->

# Releasing

Releases are built by `.github/workflows/release.yml`. It never publishes anything: a tag push
ends in a **draft** release that a maintainer reviews and publishes by hand.

RequestTailor is distributed as the web app and on Apple platforms. A release has the macOS
desktop app and the `rt` command line for macOS and Linux (for pipelines, the GitHub Action
and the Docker image). There are no Windows or Linux app bundles and no Windows code signing:
on Windows and Linux, people use the web app or `rt`. CI still builds and tests the desktop
app on macOS, Windows and Linux for every commit.

| Trigger | What happens |
| --- | --- |
| Push of a tag `v<version>` (`v0.1.0`, `v0.2.0-rc.1`; not the action's major tag `v1`) | Checks that the tag equals `[workspace.package] version` in `Cargo.toml` and that every signing secret is set, builds and signs everything, writes `SHA256SUMS` and `SHA256SUMS.asc`, creates a draft release with all files |
| Actions > Release > Run workflow | Dry run: the same build, kept as a workflow artifact (`release-dry-run-<version>`), no release. With `signed` off it needs no secrets at all |
| Actions > Release > Run workflow on the tag `v<version>`, `push-image` on | Only the Docker image: built for amd64 and arm64 from `docker/Dockerfile` and pushed to `ghcr.io/<owner>/rt:<version>` (and `latest` for a version without a pre-release suffix). Refused on anything but the release tag. Nothing else runs |

What a release contains:

| File | Built on | Signed with |
| --- | --- | --- |
| `RequestTailor_<v>_macos_universal.dmg` | macOS (Apple silicon and Intel in one app) | Developer ID, notarized and stapled (app and disk image) |
| `rt_<v>_macos_universal.zip` | macOS | Developer ID, notarized |
| `rt_<v>_linux_x86_64.tar.gz`, `rt_<v>_linux_aarch64.tar.gz` | Ubuntu 24.04, x64 and Arm runners; static musl binaries that run on any Linux distribution | through `SHA256SUMS.asc` |
| `SHA256SUMS`, `SHA256SUMS.asc`, `release-key.asc` | | GPG release key |

Each `rt` archive holds one folder `rt_<v>_<os>_<arch>/` with `rt`, `LICENSE` and `NOTICE`.
The GitHub Action (`github-action/`) downloads exactly these file names and checks them
against `SHA256SUMS`; the job "checksums and draft release" fails if one is missing. The Arm
runner (`ubuntu-24.04-arm`) is free for public repositories; for a private one it needs a
plan with Arm runners.

There is no updater. The app never checks for updates (product principle 2: no connection
the user did not ask for), so there is no update manifest, no updater key and no update
server. Users learn about releases from GitHub (watch, or the `releases.atom` feed).

Signing material is never committed. It lives only in GitHub secrets; the workflow writes it
to the runner's temporary folder, uses it, and deletes it at the end of the job.

## One-time setup

All secrets go to Settings > Secrets and variables > Actions > **Secrets**. A signed run checks
them first and fails with a list of what is missing.

### Secrets

| Name | Kind | For | Value |
| --- | --- | --- | --- |
| `APPLE_CERTIFICATE` | secret | macOS | The Developer ID Application certificate with its private key, as `.p12`, base64-encoded |
| `APPLE_CERTIFICATE_PASSWORD` | secret | macOS | The password of that `.p12` |
| `APPLE_SIGNING_IDENTITY` | secret | macOS | The certificate's name, e.g. `Developer ID Application: Example GmbH (AB12CD34EF)` |
| `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | secrets | notarization, option 1 | Apple ID email, an app-specific password for it, the 10-character team ID |
| `APPLE_API_ISSUER`, `APPLE_API_KEY`, `APPLE_API_KEY_P8` | secrets | notarization, option 2 | App Store Connect API key: issuer ID, key ID, and the text of the `.p8` file |
| `GPG_PRIVATE_KEY`, `GPG_PASSPHRASE` | secrets | checksums | The release signing key (ASCII-armored secret key) and its passphrase |

Base64 without line breaks: `base64 -i file.p12 | pbcopy` (macOS).

The Docker image needs no secret: the job pushes with the workflow's own `GITHUB_TOKEN`
(`packages: write`). The first push creates the package `rt` under the owner's packages; set
its visibility to public there and link it to the repository.

Optional hardening: move the secrets into a GitHub environment (for example `release`) that
only tags matching `v*` may use and that needs a reviewer, and add `environment: release` to
the `macos` and `release` jobs (and `image` for the push).

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

### GPG release key

The key signs `SHA256SUMS`, which covers every file, including the Linux `rt` archives.

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
a Mac that has never seen the app, with files downloaded through a browser (so they carry the
quarantine flag).

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

### Linux `rt` and the checksums

```sh
gpg --import release-key.asc
gpg --verify SHA256SUMS.asc SHA256SUMS          # "Good signature", fingerprint as published
sha256sum -c SHA256SUMS --ignore-missing
tar -xzf rt_<v>_linux_x86_64.tar.gz && ./rt_<v>_linux_x86_64/rt --version
```

### Docker image

After a run with `push-image` on:

```sh
docker run --rm ghcr.io/<owner>/rt:<v> --version
docker buildx imagetools inspect ghcr.io/<owner>/rt:<v>   # linux/amd64 and linux/arm64
```

## Not included (yet)

- Auto-update (deliberately, see above).
- Desktop app bundles for Windows and Linux (distribution decision: web app and Apple), and
  `rt` builds for Windows.
- Package managers (Homebrew, apt repository).
- Build provenance attestations and an SBOM.
