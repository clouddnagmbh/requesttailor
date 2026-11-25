// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `AccessApi` of the mock backend (wave 19, strand D): a small mirror of `rt_app::access` for UI
// tests (`?backend=mock`). The mock has no server with owners, so it plays one: user A always
// gets 200 and an object `{id, owner}`; user B gets 200 with the same object when the URL holds
// the text `leak`, 500 when it holds `crash`, a 200 that is not the object for `decoy`, and 403
// otherwise. The rules around it are the core's: a writing request needs `allow_writes` and is
// never checked against a production environment, an object reference is a `{{variable}}` in
// the path or query (else "no object reference found"), `b_must_be_denied` switches on
// `broken-function-access`, and the evidence holds paths and types, never values.

import { BackendError, type AccessApi } from "./backend";
import { mockEffect } from "./mock-agents";
import type { AccessCheck, AccessEvidence, AccessFinding, AccessVerdict, AgentPolicy, RequestSpec } from "./types";

export interface MockAccessHost {
  getRequest(path: string): Promise<RequestSpec>;
  policy(): Promise<AgentPolicy>;
  environments(): Promise<{ name: string }[]>;
}

const looksLikeProduction = (name: string) => /prod|prd|\blive\b/i.test(name);

/** The part of a URL template after the host. */
function afterAuthority(url: string): string {
  const i = url.indexOf("://");
  const rest = i >= 0 ? url.slice(i + 3) : url;
  const at = rest.search(/[/?]/);
  return at < 0 ? "" : rest.slice(at);
}

const reference = (spec: RequestSpec) => /\{\{\s*[\w.-]+\s*\}\}/.test(afterAuthority(spec.url));

const OBJECT_VALUES: [string, string][] = [
  ["$.id", "1"],
  ["$.owner", "string"],
];

function evidence(profile: "A" | "B", status: number, object: boolean): AccessEvidence {
  return {
    profile,
    status,
    header_names: ["content-type"],
    values: object ? OBJECT_VALUES : status === 200 ? [["$.message", "string"]] : [["$.error", "string"]],
  };
}

function skipped(path: string, reason: string, status_a: number | null = null): AccessCheck {
  const verdict: AccessVerdict = { kind: "skipped", reason };
  return { path, verdict, findings: [{ verdict, message: reason, evidence: [] }], status_a, status_b: null };
}

export function mockAccess(host: MockAccessHost): AccessApi {
  return {
    check: async (path, profile, options) => {
      if (profile.a === profile.b) throw new BackendError("shell", "the access check needs two different environments");
      const names = (await host.environments()).map((e) => e.name);
      for (const n of [profile.a, profile.b]) {
        if (!names.includes(n)) throw new BackendError("environment-not-found", n);
      }
      const spec = await host.getRequest(path);
      if (mockEffect(spec) === "write") {
        const policy = await host.policy();
        const production = (name: string) => {
          const entry = policy.environments.find((e) => e.environment === name);
          return entry ? entry.production : looksLikeProduction(name);
        };
        if (production(profile.a) || production(profile.b)) return skipped(path, "skipped (production)");
        if (!options.allow_writes) return skipped(path, "skipped (writes)");
      }
      const refs = reference(spec);
      if (!refs && !options.b_must_be_denied) return skipped(path, "no object reference found", 200);
      const url = spec.url;
      const status_b: number = url.includes("leak") ? 200 : url.includes("crash") ? 500 : url.includes("decoy") ? 200 : 403;
      const object = url.includes("leak");
      const both = [evidence("A", 200, true), evidence("B", status_b, object)];
      const findings: AccessFinding[] = [];
      if (refs && object) {
        findings.push({ verdict: { kind: "broken-object-access" }, message: "B received object 1 that A's request returned", evidence: both });
      }
      if (options.b_must_be_denied && status_b >= 200 && status_b < 300) {
        findings.push({
          verdict: { kind: "broken-function-access" },
          message: `B must be denied on ${spec.method.toUpperCase()} but got status ${status_b}`,
          evidence: both,
        });
      }
      let verdict: AccessVerdict;
      if (findings.length > 0) {
        verdict = findings[0]!.verdict;
      } else if (status_b === 403 || status_b === 404 || status_b === 401) {
        verdict = { kind: "ok" };
        findings.push({ verdict, message: `B was denied (status ${status_b})`, evidence: both });
      } else {
        verdict = { kind: "inconclusive" };
        findings.push({
          verdict,
          message: status_b >= 300 && status_b < 400 ? "B got a redirect" : status_b >= 400 ? `B got status ${status_b}, which says neither allowed nor denied` : "B got a success answer that does not look like A's object",
          evidence: both,
        });
      }
      return { path, verdict, findings, status_a: 200, status_b };
    },
  };
}
