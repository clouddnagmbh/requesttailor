// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `JournalApi` of the mock backend (wave 19, strand C): the placeholder until the strand replaces it.

import type { JournalApi } from "./backend";
import { stubJournal } from "./wave19-stubs";

export function mockJournal(): JournalApi {
  return stubJournal();
}
