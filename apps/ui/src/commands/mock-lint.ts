// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `LintApi` of the mock backend (wave 18, strand F): the four rules of `rt_app::lint` on the mock's
// model in memory, with the same score formula (tests 40, hosts 25, variables 15, names 20). It is
// a simplified mirror for UI tests (`?backend=mock`): the core's script scan is cruder here (any
// `vars`/`bru`/`pm` word that is not a literal access silences the variable rule). Settings live in
// memory.

import type { LintApi, VariablesApi } from "./backend";
import type {
  HealthScore,
  LintFinding,
  LintLevel,
  LintReport,
  LintRule,
  LintSettings,
  RequestSpec,
  Environment,
  RuleScore,
  ScopeVar,
} from "./types";

export interface MockLintHost {
  requests(): Map<string, RequestSpec>;
  environments(): Environment[];
  variables: VariablesApi;
  /** The name of the active collection. */
  name(): string;
}

/** The variables a folder (`""`: the collection) sees, without an environment. */
function variablesOf(host: MockLintHost): (path: string) => Promise<ScopeVar[]> {
  return async (path) => (await host.variables.scope(path, { environment: null, vars: [] })).variables;
}

const RULES: LintRule[] = ["no-tests", "hard-coded-host", "orphan-variable", "duplicate-name"];
const WEIGHT: Record<LintRule, number> = { "no-tests": 40, "hard-coded-host": 25, "orphan-variable": 15, "duplicate-name": 20 };
const DEFAULT: Record<LintRule, LintLevel> = { "no-tests": "warn", "hard-coded-host": "warn", "orphan-variable": "info", "duplicate-name": "error" };
const RANK: Record<LintLevel, number> = { off: 0, info: 1, warn: 2, error: 3 };
const id = (rule: LintRule) => `lint/${rule}`;

interface Check {
  rule: LintRule;
  folder: string;
  path: string;
  ok: boolean;
  findings: { cap?: LintLevel; message: string; location: string | null }[];
}

const parentOf = (path: string) => (path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "");

function glob(pattern: string, text: string): boolean {
  const re = new RegExp("^" + pattern.split("*").map((p) => p.replace(/[.+?^${}()|[\]\\]/g, "\\$&")).join(".*") + "$");
  return re.test(text);
}

function ignored(patterns: string[], rule: LintRule, path: string): boolean {
  return patterns.some((p) => {
    let scope: LintRule | null = null;
    let g = p;
    const m = /^lint\/([a-z-]+):(.*)$/.exec(p);
    if (m && (RULES as string[]).includes(m[1])) {
      scope = m[1] as LintRule;
      g = m[2];
    }
    if (scope && scope !== rule) return false;
    if (!g.includes("*") && g.endsWith("/")) return path.startsWith(g);
    return glob(g, path);
  });
}

type Host = { kind: "fixed" | "loopback"; host: string } | null;

function literalHost(url: string): Host {
  const m = /^([A-Za-z][A-Za-z0-9+.-]*):\/\/([^/?#]*)/.exec(url.trim());
  if (!m) return null;
  let authority = m[2];
  authority = authority.slice(authority.lastIndexOf("@") + 1);
  const host = authority.startsWith("[") ? authority.slice(1, authority.indexOf("]")) : authority.split(":")[0];
  if (!host || host.includes("{")) return null;
  const lower = host.toLowerCase();
  const loopback = lower === "localhost" || lower.endsWith(".localhost") || /^127(\.\d+){3}$/.test(lower) || lower === "::1";
  return { kind: loopback ? "loopback" : "fixed", host };
}

function urlCheck(folder: string, path: string, location: string, url: string): Check | null {
  if (url.trim() === "") return null;
  const host = literalHost(url);
  if (!host) return { rule: "hard-coded-host", folder, path, ok: true, findings: [] };
  const what = location === "url" ? "URL" : "token URL";
  return {
    rule: "hard-coded-host",
    folder,
    path,
    ok: host.kind === "loopback",
    findings: [
      host.kind === "fixed"
        ? { message: `The ${what} has a fixed host (${host.host}); use a variable such as {{baseUrl}}.`, location }
        : { cap: "info", message: `The ${what} points at ${host.host}; a variable keeps it out of the file.`, location },
    ],
  };
}

const testsIn = (scripts: { phase: string; code: string }[]) =>
  scripts.some((s) => s.code.trim() !== "" && (s.phase === "tests" || /(^|[^\w$.])test\(/.test(s.code)));

function health(counts: Map<LintRule, [number, number]>): HealthScore {
  const rules: RuleScore[] = RULES.map((rule) => ({ rule, ok: counts.get(rule)?.[0] ?? 0, total: counts.get(rule)?.[1] ?? 0 }));
  let weight = 0;
  let sum = 0;
  for (const r of rules) {
    if (r.total === 0) continue;
    weight += WEIGHT[r.rule];
    sum += (WEIGHT[r.rule] * r.ok) / r.total;
  }
  return { score: weight === 0 ? 100 : Math.max(0, Math.min(100, Math.round((100 * sum) / weight))), rules };
}

export function mockLint(host: MockLintHost): LintApi {
  let settings: LintSettings = { rules: {}, ignore: [], min_score: null };
  const level = (rule: LintRule): LintLevel => settings.rules[id(rule)] ?? DEFAULT[rule];

  async function run(): Promise<LintReport> {
    const requests: Map<string, RequestSpec> = host.requests();
    const checks: Check[] = [];
    const folders = new Set<string>();
    for (const p of requests.keys()) for (let f = parentOf(p); f !== ""; f = parentOf(f)) folders.add(f);

    if (level("no-tests") !== "off") {
      for (const [path, spec] of requests) {
        const ok = spec.assertions.some((a) => a.enabled) || testsIn(spec.scripts);
        checks.push({
          rule: "no-tests",
          folder: parentOf(path),
          path,
          ok,
          findings: ok ? [] : [{ message: "The request has no test, assertion or snapshot.", location: null }],
        });
      }
    }
    if (level("hard-coded-host") !== "off") {
      for (const [path, spec] of requests) {
        const c = urlCheck(parentOf(path), path, "url", spec.url);
        if (c) checks.push(c);
        if (spec.auth.type === "oauth2") {
          const c2 = urlCheck(parentOf(path), path, "auth.access_token_url", spec.auth.access_token_url);
          if (c2) checks.push(c2);
        }
      }
    }
    if (level("orphan-variable") !== "off") {
      const text = JSON.stringify([...requests.values(), host.environments().map((e) => e.variables.map((v) => v.value))]);
      const used = new Set<string>();
      for (const m of text.matchAll(/\{\{([^}]+)\}\}/g)) used.add(m[1].trim());
      let dynamic = false;
      for (const spec of requests.values()) {
        for (const s of spec.scripts) {
          for (const m of s.code.matchAll(/(?:rt\.vars|bru|pm\.\w+)\.\w+\(\s*(["'`])([^"'`$\\]*)\1\s*[,)]/g)) used.add(m[2]);
          const stripped = s.code.replace(/(?:rt\.vars|bru|pm\.\w+)\.\w+\(\s*(["'`])[^"'`$\\]*\1\s*[,)]/g, "");
          if (/\b(rt|bru|pm|vars|insomnia)\b/.test(stripped)) dynamic = true;
        }
        for (const a of spec.assertions) used.add(a.expression.trim());
      }
      const defined: { name: string; secret: boolean; folder: string; path: string; place: string }[] = [];
      const scope = variablesOf(host);
      for (const f of ["", ...[...folders].sort()]) {
        const vars = await scope(f);
        for (const v of vars) {
          if (f === "" && v.source.layer === "collection") defined.push({ name: v.name, secret: v.secret, folder: "", path: "", place: "the collection" });
          if (f !== "" && v.source.layer === "folder" && v.source.path === f)
            defined.push({ name: v.name, secret: v.secret, folder: f, path: `${f}/`, place: `the folder ${f.split("/").pop()}` });
        }
      }
      for (const env of host.environments()) {
        for (const v of env.variables.filter((x) => x.enabled))
          defined.push({ name: v.name, secret: v.secret, folder: "", path: `environments/${env.name}.yml`, place: `the environment ${env.name}` });
      }
      for (const d of defined) {
        const unused = !dynamic && !used.has(d.name);
        checks.push({
          rule: "orphan-variable",
          folder: d.folder,
          path: d.path,
          ok: !unused,
          findings: unused ? [{ message: `${d.secret ? "Secret" : "Variable"} \`${d.name}\` of ${d.place} is never used.`, location: `variable ${d.name}` }] : [],
        });
      }
    }
    if (level("duplicate-name") !== "off") {
      const siblings = new Map<string, { path: string; name: string; file: string }[]>();
      for (const [path, spec] of requests) {
        const list = siblings.get(parentOf(path)) ?? [];
        list.push({ path, name: spec.name.trim(), file: path.split("/").pop()!.toLowerCase() });
        siblings.set(parentOf(path), list);
      }
      for (const [folder, list] of siblings) {
        for (const item of list) {
          const findings: Check["findings"] = [];
          if (list.filter((o) => o.name === item.name).length > 1)
            findings.push({ message: `${list.filter((o) => o.name === item.name).length - 1} others in this ${folder === "" ? "collection" : "folder"} have the same name “${item.name}”.`, location: "name" });
          if (list.filter((o) => o.file === item.file).length > 1)
            findings.push({ message: `Others in this folder have the same file name “${item.file}”.`, location: "file name" });
          checks.push({ rule: "duplicate-name", folder, path: item.path, ok: findings.length === 0, findings });
        }
      }
    }

    let ignoredCount = 0;
    const kept = checks.filter((c) => {
      if (!ignored(settings.ignore, c.rule, c.path)) return true;
      ignoredCount += c.findings.length;
      return false;
    });
    const findings: LintFinding[] = kept.flatMap((c) =>
      c.findings.map((f) => {
        const base = level(c.rule);
        const lvl = f.cap && RANK[f.cap] < RANK[base] ? f.cap : base;
        return { rule: c.rule, level: lvl, path: c.path, message: f.message, location: f.location };
      }),
    );
    findings.sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : RULES.indexOf(a.rule) - RULES.indexOf(b.rule)));

    const tally = (inside: (folder: string) => boolean) => {
      const counts = new Map<LintRule, [number, number]>();
      for (const c of kept) {
        if (!inside(c.folder)) continue;
        const entry = counts.get(c.rule) ?? [0, 0];
        entry[1] += 1;
        if (c.ok) entry[0] += 1;
        counts.set(c.rule, entry);
      }
      return health(counts);
    };
    const all = ["", ...[...folders].sort()];
    return {
      collection: host.name(),
      score: tally(() => true),
      folders: all.map((folder) => ({ folder, score: tally((f) => folder === "" || f === folder || f.startsWith(`${folder}/`)) })),
      findings,
      ignored: ignoredCount,
    };
  }

  return {
    run,
    async settings() {
      return structuredClone(settings);
    },
    async setSettings(next) {
      if (next.min_score != null && next.min_score > 100) throw new Error("min_score is at most 100");
      settings = structuredClone(next);
    },
    async ignore(path, rule) {
      const entry = `${id(rule)}:${path}`;
      if (!settings.ignore.includes(entry)) settings = { ...settings, ignore: [...settings.ignore, entry] };
    },
  };
}
