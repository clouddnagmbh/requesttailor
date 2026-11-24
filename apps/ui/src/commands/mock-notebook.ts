// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `NotebookApi` of the mock backend (wave 19, strand W): the placeholder until the strand replaces it.

import type { NotebookApi } from "./backend";
import { stubNotebook } from "./wave19-stubs";

export function mockNotebook(): NotebookApi {
  return stubNotebook();
}
