// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ShrinkApi` of the mock backend (wave 19, strand E): a small mirror of `rt_app::shrink` for UI
// tests (`?backend=mock`). The mock has no server that fails, so it plays one: a probe fails
// with status 500 and the body `{"error":"boom"}` whenever the request still holds the text
// `boom` (in the URL, a header or the body); anything else answers 200 `{}`. The search is the
// same ddmin over headers, query parameters and a JSON body tree, with the same limits, the same
// confirmation of writes and the same refusal against production as the core.

import { BackendError, type ShrinkApi } from "./backend";
import { mockEffect } from "./mock-agents";
import type { AgentPolicy, RequestSpec, ShrinkPredicate, ShrinkRequest, ShrinkResult, ShrinkStep } from "./types";

export interface MockShrinkHost {
  getRequest(path: string): Promise<RequestSpec>;
  policy(): Promise<AgentPolicy>;
}

const DEFAULT_PROBES = 100;
const MAX_PROBES = 500;
const MAX_PARTS = 2000;
const MAX_DEPTH = 8;

const looksLikeProduction = (name: string) => /prod|prd|\blive\b/i.test(name);
const PROTECTED = /^(host|content-length|x-api-key|api-key|apikey|x-auth-token)$|authorization/i;

type Json = unknown;

interface Part {
  id: string;
  kind: "header" | "query" | "body";
  parent: number | null;
  protected: boolean;
  /** Where it sits: index of the header, of the query pair, or the JSON path. */
  at: number | (string | number)[];
}

function plan(spec: RequestSpec, keep: string[]): { parts: Part[]; body: Json | null } {
  const parts: Part[] = [];
  const names = new Map<string, number>();
  const name = (id: string) => {
    const n = (names.get(id) ?? 0) + 1;
    names.set(id, n);
    return n === 1 ? id : `${id}[${n}]`;
  };
  spec.headers.forEach((h, i) => {
    if (h.enabled) parts.push({ id: name(`header:${h.name}`), kind: "header", parent: null, protected: PROTECTED.test(h.name), at: i });
  });
  const q = spec.url.indexOf("?");
  if (q >= 0) {
    const query = spec.url.slice(q + 1).split("#")[0]!;
    query
      .split("&")
      .filter((p) => p !== "")
      .forEach((p, i) => parts.push({ id: name(`query:${p.split("=")[0]}`), kind: "query", parent: null, protected: false, at: i }));
  }
  let body: Json | null = null;
  if (spec.body.type === "raw" && spec.body.kind === "json") {
    try {
      body = JSON.parse(spec.body.data) as Json;
    } catch {
      body = null;
    }
  }
  if (body !== null && typeof body === "object") {
    const walk = (node: Json, path: string, route: (string | number)[], depth: number, parent: number | null) => {
      if (depth >= MAX_DEPTH || node === null || typeof node !== "object") return;
      const entries: [string | number, Json][] = Array.isArray(node) ? node.map((v, i) => [i, v]) : Object.entries(node as object);
      for (const [key, value] of entries) {
        if (parts.length >= MAX_PARTS) return;
        const here = Array.isArray(node) ? `${path}[${key}]` : `${path}.${key}`;
        const idx = parts.push({ id: `body:${here}`, kind: "body", parent, protected: false, at: [...route, key] }) - 1;
        walk(value, here, [...route, key], depth + 1, idx);
      }
    };
    walk(body, "$", [], 0, null);
  }
  for (const p of parts) if (keep.some((k) => k.toLowerCase() === p.id.toLowerCase() || p.id.startsWith(`${k}.`))) p.protected = true;
  for (let i = parts.length - 1; i >= 0; i--) {
    const parent = parts[i]!.parent;
    if (parts[i]!.protected && parent !== null) parts[parent]!.protected = true;
  }
  return { parts, body };
}

function effective(parts: Part[], kept: Set<number>): boolean[] {
  const eff: boolean[] = [];
  parts.forEach((p, i) => (eff[i] = (kept.has(i) || p.protected) && (p.parent === null || eff[p.parent]!)));
  return eff;
}

function build(spec: RequestSpec, parts: Part[], body: Json | null, eff: boolean[]): RequestSpec {
  const out: RequestSpec = structuredClone(spec);
  const gone = (kind: Part["kind"]) => new Set(parts.filter((p, i) => p.kind === kind && !eff[i]).map((p) => p.at as number));
  const dropHeaders = gone("header");
  out.headers = out.headers.filter((_, i) => !dropHeaders.has(i));
  const dropQuery = gone("query");
  if (dropQuery.size > 0) {
    const q = out.url.indexOf("?");
    const base = out.url.slice(0, q);
    const left = out.url
      .slice(q + 1)
      .split("&")
      .filter((p) => p !== "")
      .filter((_, i) => !dropQuery.has(i));
    out.url = left.length > 0 ? `${base}?${left.join("&")}` : base;
    out.params = out.params.filter((p) => p.kind !== "query" || left.some((l) => l.split("=")[0] === p.name));
  }
  if (body !== null && parts.some((p, i) => p.kind === "body" && !eff[i])) {
    const removed = parts.filter((p, i) => p.kind === "body" && !eff[i]).map((p) => (p.at as (string | number)[]).join("\u0000"));
    const rebuild = (node: Json, route: (string | number)[]): Json => {
      if (node === null || typeof node !== "object") return node;
      if (Array.isArray(node)) {
        return node.flatMap((v, i) => (removed.includes([...route, i].join("\u0000")) ? [] : [rebuild(v, [...route, i])]));
      }
      return Object.fromEntries(
        Object.entries(node as object).flatMap(([k, v]) => (removed.includes([...route, k].join("\u0000")) ? [] : [[k, rebuild(v, [...route, k])]])),
      );
    };
    out.body = { type: "raw", kind: "json", data: JSON.stringify(rebuild(body, []), null, 2) };
  }
  return out;
}

/** The mock server: 500 while the request holds `boom`. */
function answer(spec: RequestSpec): { status: number; body: string } {
  const text = [spec.url, ...spec.headers.filter((h) => h.enabled).map((h) => `${h.name}: ${h.value}`), spec.body.type === "raw" ? spec.body.data : ""].join("\n");
  return text.includes("boom") ? { status: 500, body: '{"error":"boom"}' } : { status: 200, body: "{}" };
}

function holds(p: ShrinkPredicate, a: { status: number; body: string }): boolean {
  switch (p.kind) {
    case "status":
      return a.status === p.status;
    case "server-error":
      return a.status >= 500 && a.status < 600;
    case "body-matches":
      return new RegExp(p.pattern).test(a.body);
    case "header-present":
      return p.name.toLowerCase() === "content-type";
    case "header-missing":
      return p.name.toLowerCase() !== "content-type";
    case "assertion-fails":
      return false;
    case "last-failure":
      return a.status >= 400;
  }
}

export function mockShrink(host?: MockShrinkHost): ShrinkApi {
  let busy = false;
  let cancelled = false;
  const need = (): MockShrinkHost => {
    if (!host) throw new BackendError("shell", "the mock minimal-case search has no backend");
    return host;
  };
  return {
    async run(request: ShrinkRequest): Promise<ShrinkResult> {
      const h = need();
      const original = await h.getRequest(request.path);
      const maxProbes = request.max_probes ? Math.min(request.max_probes, MAX_PROBES) : DEFAULT_PROBES;
      if (mockEffect(original) === "write") {
        const policy = await h.policy();
        const env = request.environment ?? null;
        const production = env !== null && (policy.environments.find((e) => e.environment === env)?.production ?? looksLikeProduction(env));
        if (production) throw new BackendError("shell", "A writing request is never shrunk against a production environment.");
        if (!request.confirmed_writes) throw new BackendError("shell", `This sends up to ${maxProbes} requests that write. Confirm to continue.`);
      }
      if (busy) throw new BackendError("shell", "a minimal-case search is already in progress");
      busy = true;
      cancelled = false;
      let probes = 0;
      let stopped: string | undefined;
      const { parts, body } = plan(original, request.keep ?? []);
      const predicate = request.predicate;
      try {
        const probe = (spec: RequestSpec): boolean | null => {
          if (cancelled) return (stopped = "cancelled"), null;
          if (probes >= maxProbes) return (stopped = "probes"), null;
          probes++;
          return holds(predicate, answer(spec));
        };
        const base = (): ShrinkResult => ({ request: original, removed: [], probes, one_minimal: false });
        const first = probe(original);
        if (first === null) return { ...base(), stopped_by: stopped };
        if (!first) return { ...base(), not_reproduced: true };
        const cache = new Map<string, boolean>();
        const test = (set: number[]): boolean | null => {
          const eff = effective(parts, new Set(set));
          const key = eff.map((e) => (e ? "1" : "0")).join("");
          const known = cache.get(key);
          if (known !== undefined) return known;
          const r = probe(build(original, parts, body, eff));
          if (r !== null) cache.set(key, r);
          return r;
        };
        let cur = parts.map((_, i) => i).filter((i) => !parts[i]!.protected);
        let minimal = false;
        search: {
          if (cur.length > 0) {
            const none = test([]);
            if (none === null) break search;
            if (none) cur = [];
          }
          let n = 2;
          while (cur.length >= 2) {
            const size = Math.ceil(cur.length / n);
            const chunks = Array.from({ length: Math.ceil(cur.length / size) }, (_, k) => cur.slice(k * size, (k + 1) * size));
            let reduced = false;
            for (const chunk of chunks) {
              const set = n === 2 ? chunk : cur.filter((i) => !chunk.includes(i));
              const r = test(set);
              if (r === null) break search;
              if (r) {
                cur = set;
                n = n === 2 ? 2 : Math.max(n - 1, 2);
                reduced = true;
                break;
              }
            }
            if (!reduced) {
              if (n >= cur.length) break;
              n = Math.min(n * 2, cur.length);
            }
          }
          for (let i = 0; i < cur.length; ) {
            const without = cur.filter((_, k) => k !== i);
            const r = test(without);
            if (r === null) break search;
            if (r) cur = without;
            else i++;
          }
          minimal = true;
        }
        const eff = effective(parts, new Set(cur));
        const removed: ShrinkStep[] = parts
          .map((p, i) => ({ p, i }))
          .filter(({ p, i }) => !eff[i] && (p.parent === null || eff[p.parent]!))
          .map(({ p }) => ({ part: p.id, kind: p.kind }));
        return {
          request: build(original, parts, body, eff),
          removed,
          probes,
          one_minimal: minimal && stopped === undefined,
          ...(stopped ? { stopped_by: stopped } : {}),
        };
      } finally {
        busy = false;
      }
    },
    async cancel() {
      cancelled = true;
    },
    async predicate() {
      throw new BackendError("shell", "The last answer did not fail.");
    },
  };
}
