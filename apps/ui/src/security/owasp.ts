// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The categories of the OWASP API Security Top 10 (2023), for the tags on the security checks.

import type { OwaspCategory } from "../commands/types";

export const OWASP_NAMES: Record<OwaspCategory, string> = {
  "API1:2023": "Broken Object Level Authorization",
  "API2:2023": "Broken Authentication",
  "API3:2023": "Broken Object Property Level Authorization",
  "API4:2023": "Unrestricted Resource Consumption",
  "API5:2023": "Broken Function Level Authorization",
  "API6:2023": "Unrestricted Access to Sensitive Business Flows",
  "API7:2023": "Server Side Request Forgery",
  "API8:2023": "Security Misconfiguration",
  "API9:2023": "Improper Inventory Management",
  "API10:2023": "Unsafe Consumption of APIs",
};

/** `API8` for `API8:2023`. */
export function owaspShort(category: OwaspCategory): string {
  return category.split(":")[0] ?? category;
}

/** `API8 Security Misconfiguration`. */
export function owaspTitle(category: OwaspCategory): string {
  return `${owaspShort(category)} ${OWASP_NAMES[category] ?? ""}`.trim();
}
