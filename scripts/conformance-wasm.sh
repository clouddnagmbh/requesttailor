#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 The RequestTailor Authors
#
# Browser conformance suite: starts rt-testserver on its default port (47811), runs the rt-wasm
# tests in headless Chrome (conformance, script cases), stops the server.
#
# Usage: scripts/conformance-wasm.sh [cargo test args...]
#   e.g. scripts/conformance-wasm.sh -- conformance_browser
# RT_CONFORMANCE_PORT=<port> uses another port (parallel checkouts); the browser tests are
# built for it (`option_env!` in testdata/conformance/harness.rs). CARGO_TARGET_DIR is honoured.
#
# Needs: wasm-bindgen-test-runner of the same version as wasm-bindgen in Cargo.lock
#   (cargo install wasm-bindgen-cli --version <v> --locked), Google Chrome, and a matching
#   chromedriver in $CHROMEDRIVER or on PATH (npx @puppeteer/browsers install chromedriver@<chrome version>).
set -euo pipefail
cd "$(dirname "$0")/.."

want=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/"/, "", $3); print $3 }' Cargo.lock)
have=$(wasm-bindgen-test-runner --version 2>/dev/null | awk '{ print $2 }' || true)
if [ "$have" != "$want" ]; then
  echo "conformance-wasm: need wasm-bindgen-test-runner $want (found: ${have:-none})." >&2
  echo "  cargo install wasm-bindgen-cli --version $want --locked" >&2
  exit 1
fi
CHROMEDRIVER=${CHROMEDRIVER:-$(command -v chromedriver || true)}
if [ -z "$CHROMEDRIVER" ] || [ ! -x "$CHROMEDRIVER" ]; then
  echo "conformance-wasm: chromedriver not found; set CHROMEDRIVER or put it on PATH." >&2
  exit 1
fi
export CHROMEDRIVER

port=${RT_CONFORMANCE_PORT:-47811}
case "$port" in
  '' | *[!0-9]*) echo "conformance-wasm: RT_CONFORMANCE_PORT must be a port number" >&2; exit 1 ;;
esac
export RT_CONFORMANCE_PORT=$port

cargo build -p rt-testserver --locked
log=$(mktemp)
"${CARGO_TARGET_DIR:-target}/debug/rt-testserver" --port "$port" >"$log" 2>&1 &
server=$!
trap 'kill "$server" 2>/dev/null || true; rm -f "$log"' EXIT
for _ in $(seq 1 100); do
  grep -q "listening" "$log" && break
  if ! kill -0 "$server" 2>/dev/null; then
    echo "conformance-wasm: rt-testserver exited:" >&2
    cat "$log" >&2
    exit 1
  fi
  sleep 0.1
done
if ! grep -q "listening" "$log"; then
  echo "conformance-wasm: rt-testserver did not report listening within 10 s" >&2
  exit 1
fi
cat "$log"

export CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner
# A hanging case fails the run instead of blocking it (seconds, whole test binary).
export WASM_BINDGEN_TEST_TIMEOUT=${WASM_BINDGEN_TEST_TIMEOUT:-120}
cargo test -p rt-wasm --target wasm32-unknown-unknown --locked "$@"
