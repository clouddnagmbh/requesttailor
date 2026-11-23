// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `CompareApi` of the mock backend (wave 12, strand C): a small model of `rt_app::compare` and
// `rt_app::bisect` over the mock's Git, so UI tests see real comparisons and searches.
//
// A run at a revision reads the revision's files (`treeAt`), takes today's environments and
// answers every request deterministically: the status is the number of a `/status/<n>` path
// (else 200), the body is JSON with the method, the resolved URL and the request's name. Every
// step gets the synthetic test "status is 2xx" and the request's `res.status` assertions, like
// the mock runner. The comparison and the halving follow the Rust code.

import { BackendError, type CompareApi, type SendInput } from "./backend";
import type { MockWave12Host } from "./mock-wave12";
import { emptySummary } from "./mock-runner";
import type {
  BisectOutcome,
  BisectRequest,
  BisectStep,
  BisectVerdict,
  BodyChange,
  CompareRequest,
  GitCommit,
  HeaderChange,
  RequestComparison,
  ResponseSnapshot,
  RevisionRun,
  RevisionRunRequest,
  RunComparison,
  RunStep,
  RunSummary,
  SendReport,
  TestChange,
} from "./types";

interface MockAssertion {
  expression: string;
  operator: string;
  value: string;
}

interface MockRequestFile {
  name: string;
  method: string;
  url: string;
  assertions: MockAssertion[];
}

const unquote = (v: string): string => {
  const t = v.trim();
  if (t.startsWith('"')) {
    try {
      return JSON.parse(t) as string;
    } catch {
      return t;
    }
  }
  return t;
};

/** Reads what the mock needs from a request file as `renderSpec` (mock-git.ts) writes it. */
export function readMockRequest(path: string, text: string): MockRequestFile | null {
  const lines = text.split("\n");
  let section = "";
  const file: MockRequestFile = { name: path, method: "GET", url: "", assertions: [] };
  let current: MockAssertion | null = null;
  for (const line of lines) {
    if (/^\S/.test(line)) {
      section = line.replace(/:.*$/, "");
      current = null;
      continue;
    }
    if (section === "info") {
      const m = /^ {2}name: (.*)$/.exec(line);
      if (m) file.name = unquote(m[1]!);
    } else if (section === "http") {
      const m = /^ {2}(method|url): (.*)$/.exec(line);
      if (m) file[m[1] as "method" | "url"] = unquote(m[2]!);
    } else if (section === "assertions") {
      const start = /^ {2}- (\w+): (.*)$/.exec(line);
      const more = /^ {4}(\w+): (.*)$/.exec(line);
      if (start) {
        current = { expression: "", operator: "", value: "" };
        file.assertions.push(current);
      }
      const kv = start ?? more;
      if (kv && current && (kv[1] === "expression" || kv[1] === "operator" || kv[1] === "value")) {
        current[kv[1]] = unquote(kv[2]!);
      }
    }
  }
  return file.url === "" ? null : file;
}

const TEST_OPS: Record<string, (actual: number, expected: number) => boolean> = {
  eq: (a, e) => a === e,
  neq: (a, e) => a !== e,
  gt: (a, e) => a > e,
  gte: (a, e) => a >= e,
  lt: (a, e) => a < e,
  lte: (a, e) => a <= e,
};

function mockReport(path: string, file: MockRequestFile, envName: string | null, vars: Map<string, string>): RunStep {
  const unresolved: string[] = [];
  const url = file.url.replace(/\{\{([^}]+)\}\}/g, (whole, name: string) => {
    const v = vars.get(name);
    if (v === undefined) unresolved.push(name);
    return v ?? whole;
  });
  const m = /\/status\/(\d{3})(?:[/?#]|$)/.exec(url);
  const status = m ? Number(m[1]) : 200;
  const body = JSON.stringify({ method: file.method, url, name: file.name }, null, 2);
  // A plausible, stable duration per URL, so the time bars of the comparison differ.
  let h = 7;
  for (const c of url) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  const total = 20 + (h % 180);
  const tests = [
    {
      name: "status is 2xx",
      passed: status >= 200 && status < 300,
      origin: "request",
      ...(status >= 200 && status < 300 ? {} : { error: `expected 2xx, got ${status}` }),
    },
  ];
  const assertions = file.assertions
    .filter((a) => a.expression === "res.status")
    .map((a) => {
      const op = Object.hasOwn(TEST_OPS, a.operator) ? TEST_OPS[a.operator] : undefined;
      return {
        expression: a.expression,
        operator: a.operator,
        expected: a.value,
        actual: String(status),
        passed: op !== undefined && op(status, Number(a.value)),
      };
    });
  const report: SendReport = {
    version: 1,
    request_path: path,
    environment: envName,
    request: { method: file.method, url, headers: [], body_size: 0 },
    response: {
      status,
      version: "HTTP/1.1",
      headers: [
        ["content-type", "application/json"],
        ["x-mock-name", file.name],
      ],
      url,
      redirects: [],
      body: { encoding: "utf8", data: body },
      size: body.length,
      truncated: false,
      timing: { dns: null, connect: null, tls: null, ttfb: Math.round(total * 0.8) },
      total,
    },
    variables: [],
    unresolved,
    error: null,
    tests,
    ...(assertions.length > 0 ? { assertions } : {}),
  };
  const passed = tests.every((t) => t.passed) && assertions.every((a) => a.passed);
  return { iteration: 0, request_path: path, request_name: file.name, report, passed, skipped: false };
}

// ---- comparing (the same rules as rt_app::compare) ----

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

const token = (key: string) => key.replace(/~/g, "~0").replace(/\//g, "~1");

export function jsonChanges(oldV: unknown, newV: unknown, pointer: string, out: BodyChange[]): void {
  if (out.length >= 200 || JSON.stringify(oldV) === JSON.stringify(newV)) return;
  const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
  if (isObj(oldV) && isObj(newV)) {
    const keys = [...new Set([...Object.keys(oldV), ...Object.keys(newV)])].sort();
    for (const key of keys) {
      const at = `${pointer}/${token(key)}`;
      if (key in oldV && key in newV) jsonChanges(oldV[key], newV[key], at, out);
      else out.push({ pointer: at, old: key in oldV ? oldV[key] : null, new: key in newV ? newV[key] : null });
    }
  } else if (Array.isArray(oldV) && Array.isArray(newV)) {
    for (let i = 0; i < Math.max(oldV.length, newV.length); i++) {
      const at = `${pointer}/${i}`;
      if (i < oldV.length && i < newV.length) jsonChanges(oldV[i], newV[i], at, out);
      else out.push({ pointer: at, old: i < oldV.length ? oldV[i] : null, new: i < newV.length ? newV[i] : null });
    }
  } else {
    out.push({ pointer, old: oldV, new: newV });
  }
}

function headerChanges(oldH: [string, string][], newH: [string, string][]): HeaderChange[] {
  const fold = (h: [string, string][]) => {
    const m = new Map<string, [string, string]>();
    for (const [name, value] of h) {
      const key = name.toLowerCase();
      if (VOLATILE.has(key)) continue;
      const have = m.get(key);
      m.set(key, have ? [have[0], `${have[1]}, ${value}`] : [name, value]);
    }
    return m;
  };
  const [a, b] = [fold(oldH), fold(newH)];
  const out: HeaderChange[] = [];
  for (const key of new Set([...a.keys(), ...b.keys()])) {
    if (a.get(key)?.[1] !== b.get(key)?.[1]) {
      out.push({ name: (b.get(key) ?? a.get(key))![0], old: a.get(key)?.[1] ?? null, new: b.get(key)?.[1] ?? null });
    }
  }
  return out.sort((x, y) => x.name.toLowerCase().localeCompare(y.name.toLowerCase()));
}

function outcomes(step: RunStep | undefined): Map<string, boolean> {
  const m = new Map<string, boolean>();
  for (const t of step?.report.tests ?? []) m.set(t.name, t.passed);
  for (const a of step?.report.assertions ?? []) {
    m.set(a.expected === "" ? `${a.expression} ${a.operator}` : `${a.expression} ${a.operator} ${a.expected}`, a.passed);
  }
  return m;
}

/** One side of the side-by-side view, as `rt_app::compare::response_snapshot`. */
function snapshotOf(step: RunStep | undefined): ResponseSnapshot | null {
  const r = step?.report.response;
  if (!r) return null;
  const type = r.headers.find(([n]) => n.toLowerCase() === "content-type")?.[1];
  return {
    status: r.status,
    duration_ms: r.total,
    size: r.size,
    headers: r.headers,
    ...(type ? { content_type: type } : {}),
    ...(r.body.encoding === "utf8" ? { body: r.body.data.slice(0, 256 * 1024) } : {}),
    truncated: r.truncated || (r.body.encoding === "utf8" && r.body.data.length > 256 * 1024),
  };
}

export function compareSteps(paths: string[], from: RunStep[], to: RunStep[]): RequestComparison[] {
  const first = (steps: RunStep[]) => {
    const m = new Map<string, RunStep>();
    for (const s of steps) if (!m.has(s.request_path)) m.set(s.request_path, s);
    return m;
  };
  const [a, b] = [first(from), first(to)];
  return paths.map((path) => {
    const [o, n] = [a.get(path), b.get(path)];
    const [ro, rn] = [o?.report.response ?? null, n?.report.response ?? null];
    const status = (ro?.status ?? null) !== (rn?.status ?? null) ? { old: ro?.status ?? null, new: rn?.status ?? null } : null;
    const headers = ro && rn ? headerChanges(ro.headers, rn.headers) : [];
    const body: BodyChange[] = [];
    let bodyText = false;
    if (ro && rn && ro.body.data !== rn.body.data) {
      try {
        jsonChanges(JSON.parse(ro.body.data), JSON.parse(rn.body.data), "", body);
      } catch {
        bodyText = true;
      }
    }
    const [to_, tn] = [outcomes(o), outcomes(n)];
    const tests: TestChange[] = [];
    for (const name of new Set([...to_.keys(), ...tn.keys()])) {
      if (to_.get(name) !== tn.get(name)) tests.push({ name, old: to_.get(name) ?? null, new: tn.get(name) ?? null });
    }
    const same = !!o === !!n && !status && headers.length === 0 && body.length === 0 && !bodyText && tests.length === 0;
    return {
      path,
      name: n?.request_name ?? o?.request_name ?? path,
      same,
      status,
      headers,
      body,
      body_text: bodyText,
      tests,
      error: null,
      sides: { old: snapshotOf(o), new: snapshotOf(n) },
    };
  });
}

// ---- bisect (the same halving as rt_app::bisect::search) ----

export async function searchBisect(
  candidatesNewestFirst: GitCommit[],
  test: (commit: GitCommit) => Promise<[BisectVerdict, string[]]>,
  onStep: (step: BisectStep) => void,
  cancelled: () => boolean,
): Promise<BisectOutcome> {
  const commits = [...candidatesNewestFirst].reverse();
  const steps: BisectStep[] = [];
  const done = (first: number | null): BisectOutcome => ({
    first_bad: first === null ? null : commits[first]!,
    candidates: commits.length,
    steps,
  });
  const record = (i: number, verdict: BisectVerdict, failed: string[], remaining: number) => {
    const step = { commit: commits[i]!, verdict, failed, remaining };
    onStep(step);
    steps.push(step);
  };
  if (commits.length === 0 || cancelled()) return done(null);
  const tip = commits.length - 1;
  const [tipVerdict, tipFailed] = await test(commits[tip]!);
  if (cancelled()) return done(null);
  record(tip, tipVerdict, tipFailed, tip);
  if (tipVerdict !== "bad") return done(null);
  let lo = -1;
  let hi = tip;
  const skipped = new Set<number>();
  for (;;) {
    const start = lo + 1;
    if (start >= hi) break;
    if (cancelled()) return done(null);
    const mid = start + Math.floor((hi - start) / 2);
    let pick = -1;
    for (let d = 0; d < hi - start && pick < 0; d++) {
      for (const i of [mid - d, mid + d]) {
        if (i >= start && i < hi && !skipped.has(i)) {
          pick = i;
          break;
        }
      }
    }
    if (pick < 0) return done(null);
    const [verdict, failed] = await test(commits[pick]!);
    if (cancelled()) return done(null);
    if (verdict === "good") lo = pick;
    else if (verdict === "bad") hi = pick;
    else skipped.add(pick);
    let remaining = 0;
    for (let i = lo + 1; i < hi; i++) if (!skipped.has(i)) remaining++;
    record(pick, verdict, failed, remaining);
  }
  for (let i = lo + 1; i < hi; i++) if (skipped.has(i)) return done(null);
  return done(hi);
}

function failureText(step: RunStep): string {
  const a = step.report.assertions?.find((x) => !x.passed);
  if (a) return `${step.request_name}: ${a.expression} ${a.operator} ${a.expected} (got ${a.actual ?? "nothing"})`;
  const t = step.report.tests?.find((x) => !x.passed);
  return `${step.request_name}: ${t ? (t.error ?? t.name) : "failed"}`;
}

export function mockCompare(host: MockWave12Host): CompareApi {
  let active: { cancelled: boolean } | null = null;

  const begin = () => {
    if (active) throw new BackendError("shell", "a compare or bisect is already in progress");
    active = { cancelled: false };
    return active;
  };

  const commitOf = async (rev: string): Promise<GitCommit> => {
    const [commit] = await host.git().commits(null, rev, 1);
    if (!commit) throw new BackendError("git", `${rev} does not name a commit`);
    return commit;
  };

  /** The runnable requests of `rev` in path order: `[path, file]`. */
  const filesAt = async (rev: string): Promise<[string, MockRequestFile][]> => {
    const tree = await host.git().treeAt(rev);
    return Object.entries(tree.files)
      .sort(([x], [y]) => x.localeCompare(y))
      .flatMap(([path, text]): [string, MockRequestFile][] => {
        const file = readMockRequest(path, text);
        return file ? [[path, file]] : [];
      });
  };

  const varsOf = (input: SendInput): Map<string, string> => {
    const env = host.environments().find((e) => e.name === input.environment);
    const vars = new Map<string, string>();
    for (const v of env?.variables ?? []) if (v.enabled && !v.secret && v.value !== null) vars.set(v.name, v.value);
    for (const [n, v] of input.vars) vars.set(n, v);
    return vars;
  };

  const runFiles = (
    files: [string, MockRequestFile][],
    input: SendInput,
    folder: string | null | undefined,
    paths: string[] | null | undefined,
  ): { steps: RunStep[]; summary: RunSummary } => {
    const prefix = folder ? `${folder.replace(/\/+$/, "")}/` : "";
    const wanted = paths
      ? paths.flatMap((p) => {
          const f = files.find(([x]) => x === p);
          return f ? [f] : [];
        })
      : files.filter(([p]) => p.startsWith(prefix));
    const vars = varsOf(input);
    const summary = emptySummary();
    const steps = wanted.map(([path, file]) => mockReport(path, file, input.environment, vars));
    for (const s of steps) {
      summary.requests++;
      if (s.passed) summary.passed++;
      else summary.failed++;
      for (const t of s.report.tests ?? []) t.passed ? summary.tests_passed++ : summary.tests_failed++;
      for (const a of s.report.assertions ?? []) a.passed ? summary.assertions_passed++ : summary.assertions_failed++;
    }
    return { steps, summary };
  };

  const tick = () => new Promise((r) => setTimeout(r, 0));

  return {
    async run(request: RevisionRunRequest, input): Promise<RevisionRun> {
      begin();
      try {
        const commit = await commitOf(request.rev);
        const files = await filesAt(request.rev);
        await tick();
        return { commit, ...runFiles(files, input, request.folder, request.paths) };
      } finally {
        active = null;
      }
    },

    async compare(request: CompareRequest, input): Promise<RunComparison> {
      begin();
      try {
        const [fromCommit, toCommit] = [await commitOf(request.from), await commitOf(request.to)];
        const [fromFiles, toFiles] = [await filesAt(request.from), await filesAt(request.to)];
        const prefix = request.folder ? `${request.folder.replace(/\/+$/, "")}/` : "";
        const paths =
          request.paths ??
          [...new Set([...fromFiles.map(([p]) => p), ...toFiles.map(([p]) => p)])].filter((p) => p.startsWith(prefix));
        await tick();
        const before = runFiles(fromFiles, input, null, paths);
        const after = runFiles(toFiles, input, null, paths);
        return {
          from: fromCommit,
          to: toCommit,
          requests: compareSteps(paths, before.steps, after.steps),
          from_summary: before.summary,
          to_summary: after.summary,
        };
      } finally {
        active = null;
      }
    },

    async bisect(request: BisectRequest, input, onStep): Promise<BisectOutcome> {
      const token_ = begin();
      try {
        const candidates = await host.git().commits(request.good, request.bad, 1_000_000);
        const folder = request.folder || null;
        return await searchBisect(
          candidates,
          async (commit) => {
            await tick();
            let files: [string, MockRequestFile][];
            try {
              files = await filesAt(commit.oid);
            } catch {
              return ["skip", []];
            }
            const run = runFiles(files, input, folder, null);
            if (run.steps.length === 0) return ["skip", []];
            const failed = run.steps.filter((s) => !s.passed).map(failureText);
            return [failed.length > 0 ? "bad" : "good", failed];
          },
          onStep,
          () => token_.cancelled,
        );
      } finally {
        active = null;
      }
    },

    async cancel() {
      if (active) active.cancelled = true;
    },
  };
}
