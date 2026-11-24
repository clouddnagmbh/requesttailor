// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `EnvCompareApi` of the mock backend (wave 18, strand A): a small model of
// `rt_app::envcompare` over the mock's own sends, so UI tests see real comparisons. Reading
// requests only (the effect comes from the mock's mirror of `rt_core::guard::effect`), one
// request at a time through all environments, the first environment is the reference,
// production needs `production_confirmed`, volatile headers are left out, and with
// `ignore_env_values` the environment's own values become `{{name}}`.

import { BackendError, type EnvCompareApi, type SendInput } from "./backend";
import { mockEffect } from "./mock-agents";
import { planOfTree } from "./mock-runner";
import type {
  AgentPolicy,
  BodyChange,
  Environment,
  HeaderChange,
  RequestComparison,
  RequestSpec,
  ResponseSnapshot,
  RunSummary,
  StoredResponse,
  TreeNode,
} from "./types";

/** What the mock comparison uses of the mock backend. */
export interface MockEnvCompareHost {
  tree(): Promise<TreeNode>;
  getRequest(path: string): Promise<RequestSpec>;
  environments(): Promise<Environment[]>;
  policy(): Promise<AgentPolicy>;
  send(path: string, input: SendInput): Promise<StoredResponse>;
  releaseResponse(id: number): Promise<void>;
}

const VOLATILE = new Set([
  "date",
  "age",
  "server-timing",
  "x-request-id",
  "x-correlation-id",
  "x-trace-id",
  "traceparent",
  "content-length",
  "keep-alive",
  "connection",
]);

const MIN_IGNORED_VALUE = 4;

const looksLikeProduction = (name: string) => /prod|prd|\blive\b/i.test(name);

function summary(): RunSummary {
  return {
    requests: 0,
    passed: 0,
    failed: 0,
    skipped: 0,
    tests_passed: 0,
    tests_failed: 0,
    assertions_passed: 0,
    assertions_failed: 0,
    duration_ms: 0,
    stopped: false,
  };
}

function diffJson(a: unknown, b: unknown, pointer: string, out: BodyChange[]): void {
  if (JSON.stringify(a) === JSON.stringify(b)) return;
  const object = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
  if (object(a) && object(b)) {
    for (const key of [...new Set([...Object.keys(a), ...Object.keys(b)])].sort()) {
      const at = `${pointer}/${key.replace(/~/g, "~0").replace(/\//g, "~1")}`;
      if (key in a && key in b) diffJson(a[key], b[key], at, out);
      else out.push({ pointer: at, old: key in a ? a[key] : null, new: key in b ? b[key] : null });
    }
  } else if (Array.isArray(a) && Array.isArray(b)) {
    for (let i = 0; i < Math.max(a.length, b.length); i++) {
      if (i < a.length && i < b.length) diffJson(a[i], b[i], `${pointer}/${i}`, out);
      else out.push({ pointer: `${pointer}/${i}`, old: a[i] ?? null, new: b[i] ?? null });
    }
  } else out.push({ pointer, old: a, new: b });
}

function snapshot(report: StoredResponse["report"], swap: (s: string) => string): ResponseSnapshot | null {
  const r = report.response;
  if (!r) return null;
  const headers = r.headers.map(([n, v]): [string, string] => [n, swap(v)]);
  const type = headers.find(([n]) => n.toLowerCase() === "content-type")?.[1];
  return {
    status: r.status,
    duration_ms: r.total,
    size: r.size,
    headers,
    ...(type ? { content_type: type } : {}),
    ...(r.body.encoding === "utf8" ? { body: swap(r.body.data) } : {}),
    truncated: r.truncated,
  };
}

function compare(path: string, name: string, old: ResponseSnapshot | null, now: ResponseSnapshot | null): RequestComparison {
  const status = old?.status === now?.status ? null : { old: old?.status ?? null, new: now?.status ?? null };
  const headers: HeaderChange[] = [];
  const body: BodyChange[] = [];
  let bodyText = false;
  if (old && now) {
    const fold = (h: [string, string][]) => new Map(h.filter(([n]) => !VOLATILE.has(n.toLowerCase())).map(([n, v]) => [n.toLowerCase(), [n, v] as const]));
    const [a, b] = [fold(old.headers), fold(now.headers)];
    for (const key of [...new Set([...a.keys(), ...b.keys()])].sort()) {
      if (a.get(key)?.[1] !== b.get(key)?.[1]) {
        headers.push({ name: (b.get(key) ?? a.get(key))![0], old: a.get(key)?.[1] ?? null, new: b.get(key)?.[1] ?? null });
      }
    }
    if (old.body !== now.body) {
      try {
        diffJson(JSON.parse(old.body ?? ""), JSON.parse(now.body ?? ""), "", body);
      } catch {
        bodyText = true;
      }
    }
  }
  const same = !status && headers.length === 0 && body.length === 0 && !bodyText && (old === null) === (now === null);
  return { path, name, same, status, headers, body, body_text: bodyText, tests: [], error: null, sides: { old, new: now } };
}

export function mockEnvCompare(host?: MockEnvCompareHost): EnvCompareApi {
  let busy = false;
  const need = (): MockEnvCompareHost => {
    if (!host) throw new BackendError("shell", "the mock environment comparison has no backend");
    return host;
  };
  return {
    async run(request) {
      const h = need();
      const names = request.environments;
      if (names.length < 2) throw new BackendError("shell", "comparing environments needs at least two environments");
      if (new Set(names).size !== names.length) throw new BackendError("shell", "an environment is listed twice");
      const envs = await h.environments();
      for (const n of names) if (!envs.some((e) => e.name === n)) throw new BackendError("environment-not-found", n);
      const policy = await h.policy();
      const isProd = (n: string) => policy.environments.find((e) => e.environment === n)?.production ?? looksLikeProduction(n);
      const prod = names.find(isProd);
      if (prod && !request.production_confirmed) {
        throw new BackendError("shell", `${prod} is a production environment: sending its reads needs confirmation`);
      }
      if (busy) throw new BackendError("shell", "an environment comparison is already in progress");
      busy = true;
      try {
        const folder = (request.folder ?? "").replace(/\/+$/, "");
        const paths = request.paths ?? planOfTree(await h.tree(), folder === "" ? null : folder);
        const swaps = names.map((n) => {
          if (!request.ignore_env_values) return (s: string) => s;
          const values = (envs.find((e) => e.name === n)?.variables ?? [])
            .filter((v) => v.enabled && !v.secret && (v.value ?? "").length >= MIN_IGNORED_VALUE)
            .sort((x, y) => y.value!.length - x.value!.length);
          return (s: string) => values.reduce((t, v) => t.split(v.value!).join(`{{${v.name}}}`), s);
        });
        const skipped: { path: string; name: string; reason: string }[] = [];
        const sent: string[] = [];
        const names_: Record<string, string> = {};
        const sides = names.map(() => new Map<string, ResponseSnapshot | null>());
        const sums = names.map(() => summary());
        for (const path of paths) {
          const spec = await h.getRequest(path);
          if (mockEffect(spec) === "write") {
            skipped.push({ path, name: spec.name, reason: "skipped (writes)" });
            continue;
          }
          sent.push(path);
          names_[path] = spec.name;
          for (let i = 0; i < names.length; i++) {
            const result = await h.send(path, { environment: names[i]!, vars: [] });
            sides[i]!.set(path, snapshot(result.report, swaps[i]!));
            if (result.response_id != null) await h.releaseResponse(result.response_id);
            const ok = result.report.error === null && (result.report.response?.status ?? 500) < 400;
            sums[i]!.requests++;
            if (ok) sums[i]!.passed++;
            else sums[i]!.failed++;
          }
        }
        return {
          reference: names[0]!,
          pairs: names.slice(1).map((environment, k) => ({
            environment,
            requests: sent.map((p) => compare(p, names_[p]!, sides[0]!.get(p) ?? null, sides[k + 1]!.get(p) ?? null)),
            reference_summary: sums[0]!,
            summary: sums[k + 1]!,
          })),
          skipped,
        };
      } finally {
        busy = false;
      }
    },
  };
}
