// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `SequenceApi` of the web app (wave 19, strand G): the placeholder of `@requesttailor/ui` until the
// strand answers it over rt-wasm (`crates/rt-wasm/src/sequence_js.rs`).

import { stubSequence, type SequenceApi } from "@requesttailor/ui";

export function webSequence(): SequenceApi {
  return stubSequence();
}
