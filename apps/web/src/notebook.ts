// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `NotebookApi` of the web app (wave 19, strand W): the placeholder of `@requesttailor/ui` until the
// strand answers it over rt-wasm (`crates/rt-wasm/src/notebook_js.rs`).

import { stubNotebook, type NotebookApi } from "@requesttailor/ui";

export function webNotebook(): NotebookApi {
  return stubNotebook();
}
