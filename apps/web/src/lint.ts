// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `LintApi` of the web app (wave 18, strand F): the placeholder of `@requesttailor/ui` until the
// strand answers it over rt-wasm (`crates/rt-wasm/src/lint_js.rs`).

import { stubLint, type LintApi } from "@requesttailor/ui";

export function webLint(): LintApi {
  return stubLint();
}
