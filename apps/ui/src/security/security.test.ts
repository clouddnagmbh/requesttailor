// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { MockBackend } from "../commands/mock";
import { MOCK_SECURITY_RULES } from "../commands/mock-security";
import { OWASP_NAMES, owaspShort, owaspTitle } from "./owasp";
import { securityRules } from "./rules";

/** The core's `rt_app::security::rules()` ids, in order (`security_passive.rs` has the same list). */
const CORE_IDS = [
  "chk/cors-credentials",
  "sec/cors-reflected-origin",
  "chk/jwt-alg-none",
  "sec/jwt-no-exp",
  "sec/jwt-expired-in-use",
  "sec/jwt-weak-hmac",
  "chk/hsts-missing",
  "chk/nosniff-missing",
  "sec/cache-control-tokens",
  "sec/no-rate-limit",
  "sec/tls-old-protocol",
  "sec/tls-cert-expired",
  "sec/tls-cert-expiring",
  "sec/tls-weak-key",
  "chk/stack-trace",
  "chk/version-disclosure",
  "sec/error-page-version",
];

describe("the mock mirror of the passive security checks", () => {
  it("lists the core's rules, the TLS ones as native only", async () => {
    const backend = new MockBackend();
    const rules = await backend.security.rules();
    expect(rules.map((r) => r.id)).toEqual(CORE_IDS);
    expect(MOCK_SECURITY_RULES.map(([id]) => id)).toEqual(CORE_IDS);
    expect(rules.filter((r) => r.native_only).map((r) => r.id)).toEqual(CORE_IDS.filter((id) => id.startsWith("sec/tls-")));
    for (const r of rules) expect(OWASP_NAMES[r.category]).toBeTruthy();
  });

  it("turns the observations of audit rules into findings with a category", async () => {
    const backend = new MockBackend();
    await backend.openCollection();
    const stored = await backend.send("users/list-users.yml", { environment: null, vars: [] });
    const target = { path: "users/list-users.yml", response_id: stored.response_id! };
    const findings = await backend.security.check(target);
    const nosniff = findings.find((f) => f.rule === "chk/nosniff-missing");
    expect(nosniff?.category).toBe("API8:2023");
    expect(nosniff?.severity).toBe("low");
    // Rules that are not part of the audit stay out of the findings.
    expect(findings.every((f) => CORE_IDS.includes(f.rule))).toBe(true);

    await backend.security.setSettings({ enabled: true, disabled: ["chk/nosniff-missing"] });
    expect((await backend.security.check(target)).map((f) => f.rule)).not.toContain("chk/nosniff-missing");
    await backend.security.setSettings({ enabled: false, disabled: [] });
    expect(await backend.security.check(target)).toEqual([]);
    expect(await backend.security.settings()).toEqual({ enabled: false, disabled: [] });
  });

  it("asks the backend for the rules once and survives a refusal", async () => {
    const backend = new MockBackend();
    const spy = { n: 0 };
    const original = backend.security.rules;
    backend.security.rules = async () => {
      spy.n++;
      return original();
    };
    const a = await securityRules(backend);
    const b = await securityRules(backend);
    expect(spy.n).toBe(1);
    expect(a).toBe(b);
    expect(a.get("sec/tls-weak-key")?.category).toBe("API8:2023");

    const broken = new MockBackend();
    broken.security.rules = () => Promise.reject(new Error("no"));
    expect((await securityRules(broken)).size).toBe(0);
  });

  it("names the categories", () => {
    expect(owaspShort("API10:2023")).toBe("API10");
    expect(owaspTitle("API2:2023")).toBe("API2 Broken Authentication");
  });
});
