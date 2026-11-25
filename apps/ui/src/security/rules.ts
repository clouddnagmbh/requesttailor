// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The security rules by id, asked once per backend: the checks tab tags an observation with its
// OWASP category when the rule is one of the passive security rules. A backend that says no
// (or an older one) just leaves the tag out.

import type { Backend } from "../commands/backend";
import type { SecurityRule } from "../commands/types";

const cache = new WeakMap<Backend, Promise<Map<string, SecurityRule>>>();

export function securityRules(backend: Backend): Promise<Map<string, SecurityRule>> {
  let found = cache.get(backend);
  if (!found) {
    found = backend.security
      .rules()
      .then((rules) => new Map(rules.map((r) => [r.id, r])))
      .catch(() => new Map<string, SecurityRule>());
    cache.set(backend, found);
  }
  return found;
}
