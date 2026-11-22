#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 The RequestTailor Authors
#
# Downloads `rt` for this runner from the RequestTailor GitHub release, checks it against the
# release's SHA256SUMS and prints `path=<rt>` to $GITHUB_OUTPUT.
#
# Environment: INPUT_VERSION (`latest`, `0.1.0` or `v0.1.0`). RT_ACTION_RELEASES overrides
# the releases URL (tests use a file:// folder laid out like a release).
set -euo pipefail

releases="${RT_ACTION_RELEASES:-https://github.com/clouddnagmbh/requesttailor/releases}"
version="${INPUT_VERSION:-latest}"
tmp="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"

fail() {
  echo "::error::$1" >&2
  exit 1
}

# The release asset for an OS (`uname -s`) and machine (`uname -m`); the names are the ones
# .github/workflows/release.yml uploads.
asset_for() {
  local os="$1" machine="$2" v="$3"
  case "$os" in
    Linux)
      case "$machine" in
        x86_64 | amd64) echo "rt_${v}_linux_x86_64.tar.gz" ;;
        aarch64 | arm64) echo "rt_${v}_linux_aarch64.tar.gz" ;;
        *) return 1 ;;
      esac ;;
    Darwin) echo "rt_${v}_macos_universal.zip" ;;
    *) return 1 ;;
  esac
}

sha256() {
  if command -v sha256sum > /dev/null; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

fetch() { # url file
  curl --proto '=https,file' --tlsv1.2 -fsSL --retry 3 -o "$2" "$1"
}

if [ "$version" = "latest" ]; then
  if [ -n "${RT_ACTION_RELEASES:-}" ]; then
    version=$(cat "${releases#file://}/latest")
  else
    # github.com/<repo>/releases/latest redirects to .../releases/tag/v<version>.
    url=$(curl -fsSLI --retry 3 -o /dev/null -w '%{url_effective}' "$releases/latest")
    version="${url##*/}"
  fi
fi
version="${version#v}"
case "$version" in
  "" | */* | *[!0-9A-Za-z.+-]*) fail "Cannot tell the rt version from \"$version\"; set the input version, e.g. 0.1.0." ;;
esac

os=$(uname -s)
machine=$(uname -m)
asset=$(asset_for "$os" "$machine" "$version") ||
  fail "No rt build for $os $machine. Use a Linux (x86_64, aarch64) or macOS runner, or the Docker image ghcr.io/clouddnagmbh/rt."

dir="$tmp/rt-action-$version-$$"
mkdir -p "$dir"
base="$releases/download/v$version"
echo "Downloading $asset (rt $version)"
fetch "$base/$asset" "$dir/$asset" || fail "Cannot download $base/$asset. Does release v$version exist?"
fetch "$base/SHA256SUMS" "$dir/SHA256SUMS" || fail "Cannot download $base/SHA256SUMS."

expected=$(awk -v f="$asset" '$2 == f || $2 == "*" f { print $1 }' "$dir/SHA256SUMS")
[ -n "$expected" ] || fail "$asset is not listed in SHA256SUMS of v$version."
actual=$(sha256 "$dir/$asset")
[ "$expected" = "$actual" ] || fail "SHA-256 of $asset is $actual, SHA256SUMS says $expected. Not using it."
echo "SHA-256 matches SHA256SUMS: $actual"

case "$asset" in
  *.zip) unzip -q "$dir/$asset" -d "$dir" ;;
  *) tar -xzf "$dir/$asset" -C "$dir" ;;
esac
# Each archive holds one folder named like it: rt_<version>_<os>_<arch>/rt.
folder="${asset%.tar.gz}"
folder="${folder%.zip}"
rt="$dir/$folder/rt"
[ -x "$rt" ] || fail "No $folder/rt in $asset."
"$rt" --version
echo "path=$rt" >> "${GITHUB_OUTPUT:-/dev/null}"
