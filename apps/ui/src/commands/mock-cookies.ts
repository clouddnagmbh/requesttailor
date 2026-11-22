// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The cookie jar in the mock backend (wave 5, strand D owns this file). Interface commit: empty.

import type { CookieApi } from "./backend";

export function mockCookies(): CookieApi {
  return {
    list: async () => [],
    delete: async () => {},
    clear: async () => {},
  };
}
