// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `SecurityApi` of the mock backend (wave 19, strand R): a mirror of `rt_app::security` for UI
// tests (`?backend=mock`). The rule list is the core's (a test keeps both in step); the findings
// are the response checks of the mock inspector (`mock-inspect.ts`, the nine rules of wave 18)
// that belong to the audit, with the rule's category. The `sec/` rules are the core's: the mock
// has no TLS details and no token store to reproduce them. Settings live in memory.

import type { InspectApi, SecurityApi } from "./backend";
import type { OwaspCategory, SecurityFinding, SecurityRule, SecuritySettings, SecuritySeverity } from "./types";

type Row = [id: string, title: string, category: OwaspCategory, severity: SecuritySeverity, nativeOnly?: boolean];

const API2: OwaspCategory = "API2:2023";
const API4: OwaspCategory = "API4:2023";
const API8: OwaspCategory = "API8:2023";

/** The core's `rt_app::security::rules()`, in order. */
export const MOCK_SECURITY_RULES: Row[] = [
  ["chk/cors-credentials", "CORS allows any origin with credentials", API8, "medium"],
  ["sec/cors-reflected-origin", "CORS repeats the request's Origin", API8, "low"],
  ["chk/jwt-alg-none", "Token with alg none", API2, "high"],
  ["sec/jwt-no-exp", "Token without an expiry", API2, "low"],
  ["sec/jwt-expired-in-use", "Expired token sent with the request", API2, "medium"],
  ["sec/jwt-weak-hmac", "Token signed with a guessable secret", API2, "high"],
  ["chk/hsts-missing", "Strict-Transport-Security is missing", API8, "low"],
  ["chk/nosniff-missing", "X-Content-Type-Options is missing", API8, "low"],
  ["sec/cache-control-tokens", "Token in a response that may be cached", API8, "medium"],
  ["sec/no-rate-limit", "No sign of rate limiting", API4, "low"],
  ["sec/tls-old-protocol", "Outdated TLS protocol", API8, "high", true],
  ["sec/tls-cert-expired", "Certificate expired", API8, "high", true],
  ["sec/tls-cert-expiring", "Certificate expires soon", API8, "low", true],
  ["sec/tls-weak-key", "Weak certificate key", API8, "high", true],
  ["chk/stack-trace", "Stack trace in an error response", API8, "medium"],
  ["chk/version-disclosure", "Server version in a header", API8, "info"],
  ["sec/error-page-version", "Software version on an error page", API8, "info"],
];

function rules(): SecurityRule[] {
  return MOCK_SECURITY_RULES.map(([id, title, category, severity, nativeOnly]) => ({
    id,
    title,
    category,
    severity,
    ...(nativeOnly ? { native_only: true } : {}),
  }));
}

export function mockSecurity(inspect?: () => InspectApi): SecurityApi {
  let settings: SecuritySettings = { enabled: true, disabled: [] };
  return {
    check: async (target) => {
      const api = inspect?.();
      if (!api || !settings.enabled) return [];
      const known = new Map(rules().map((r) => [r.id, r]));
      const observations = await api.checks({ path: target.path, environment: null, response_id: target.response_id });
      return observations.flatMap((o): SecurityFinding[] => {
        const rule = known.get(o.id);
        if (!rule || (settings.disabled ?? []).includes(o.id)) return [];
        return [{ rule: o.id, category: rule.category, severity: rule.severity, location: o.location, why: o.why, fix: o.fix }];
      });
    },
    rules: async () => rules(),
    settings: async () => ({ enabled: settings.enabled, disabled: [...(settings.disabled ?? [])] }),
    setSettings: async (next) => {
      settings = { enabled: next.enabled !== false, disabled: [...(next.disabled ?? [])] };
    },
  };
}
