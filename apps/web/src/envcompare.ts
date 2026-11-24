// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `EnvCompareApi` of the web app (wave 18, strand A): the placeholder of `@requesttailor/ui` until the
// strand answers it over rt-wasm (`crates/rt-wasm/src/envcompare_js.rs`).

import { stubEnvCompare, type EnvCompareApi } from "@requesttailor/ui";

export function webEnvCompare(): EnvCompareApi {
  return stubEnvCompare();
}
