// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand A: comparing environments, one Tauri command per method (`src-tauri/src/envcompare.rs`,
// placeholders until strand A).

import type { EnvCompareApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriEnvCompare(call: Call): EnvCompareApi {
  return {
    run: (request) => call("envcompare_run", { request }),
  };
}
