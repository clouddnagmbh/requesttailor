// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `InboxApi` of the mock backend (wave 18, strand E): the placeholder until the strand replaces it.

import type { InboxApi } from "./backend";
import { stubInbox } from "./wave18-stubs";

export function mockInbox(): InboxApi {
  return stubInbox();
}
