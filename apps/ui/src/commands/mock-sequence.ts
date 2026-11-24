// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `SequenceApi` of the mock backend (wave 19, strand G): the placeholder until the strand replaces it.

import type { SequenceApi } from "./backend";
import { stubSequence } from "./wave19-stubs";

export function mockSequence(): SequenceApi {
  return stubSequence();
}
