// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `RunnerApi` in the mock backend (wave 7, strand D owns this file): a small model of
// `rt_core::runner` over the mock's own sends, so UI tests see real runs. Plan in tree
// order; iterations or one per data row (row values become `{{variables}}`, below the
// caller's `vars`); delay with cancel; bail; one run at a time. Every step gets one
// synthetic test, "status is 2xx", and the mock's declarative assertions on `res.status`
// (`eq`, `neq`, `gt`, `lt`) are evaluated, so passed and failed counts are meaningful.
// Response bodies in step events are cut to 4 KiB, as the real shells do.

import { mockWaitMs } from "./mock-ratelimit";
import { BackendError, type RunnerApi, type SendInput } from "./backend";
import type { AssertionResult, DataRow, RequestSpec, RunStep, RunSummary, SendReport, StoredResponse, TreeNode } from "./types";

export function emptySummary(): RunSummary {
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

/** What the mock runner uses of the mock backend. */
export interface MockRunnerHost {
  tree(): Promise<TreeNode>;
  getRequest(path: string): Promise<RequestSpec>;
  send(path: string, input: SendInput): Promise<StoredResponse>;
  releaseResponse(id: number): Promise<void>;
}

const VIEW_BODY_LIMIT = 4 * 1024;

/** Request paths below `folder` (`null`/`""`: all), depth first in tree order. */
export function planOfTree(tree: TreeNode, folder: string | null): string[] {
  const find = (node: TreeNode, path: string): TreeNode | null => {
    if (node.kind !== "folder") return null;
    if (node.path === path) return node;
    for (const c of node.children) {
      const f = find(c, path);
      if (f) return f;
    }
    return null;
  };
  const collect = (node: TreeNode): string[] =>
    node.kind === "request" ? [node.path] : node.children.flatMap(collect);
  const path = (folder ?? "").replace(/\/+$/, "");
  const start = path === "" ? tree : find(tree, path);
  return start ? collect(start) : [];
}

/** CSV (header row, quoted fields, CRLF/LF, BOM) or JSON (array of objects), like
 *  `rt_core::runner::parse_data`, with the same kind of messages. */
export function parseDataMock(name: string, data: Uint8Array): DataRow[] {
  let text: string;
  try {
    text = new TextDecoder("utf-8", { fatal: true }).decode(data);
  } catch {
    throw new BackendError("shell", `${name}: not valid UTF-8 text`);
  }
  text = text.replace(/^﻿/, "");
  const lower = name.toLowerCase();
  if (lower.endsWith(".json")) {
    let value: unknown;
    try {
      value = JSON.parse(text);
    } catch (e) {
      throw new BackendError("shell", `${name}: ${e instanceof Error ? e.message : String(e)}`);
    }
    if (!Array.isArray(value)) throw new BackendError("shell", `${name}: expected an array of objects`);
    return value.map((item, i) => {
      if (item === null || typeof item !== "object" || Array.isArray(item)) {
        throw new BackendError("shell", `${name}: item ${i + 1}: expected an object`);
      }
      return Object.entries(item as Record<string, unknown>);
    });
  }
  if (!lower.endsWith(".csv")) throw new BackendError("shell", `${name}: data files are .csv or .json`);
  const records: { line: number; fields: string[] }[] = [];
  let fields: string[] = [];
  let field = "";
  let quoted = false;
  let line = 1;
  let start = 1;
  let blank = true;
  for (let i = 0; i < text.length; i++) {
    const c = text[i]!;
    if (quoted) {
      if (c === '"' && text[i + 1] === '"') {
        field += '"';
        i++;
      } else if (c === '"') quoted = false;
      else {
        if (c === "\n") line++;
        field += c;
      }
    } else if (c === '"' && field === "") {
      quoted = true;
      blank = false;
    } else if (c === ",") {
      fields.push(field);
      field = "";
      blank = false;
    } else if (c === "\n" || c === "\r") {
      if (c === "\r" && text[i + 1] === "\n") i++;
      fields.push(field);
      if (!blank) records.push({ line: start, fields });
      fields = [];
      field = "";
      blank = true;
      line++;
      start = line;
    } else {
      field += c;
      blank = false;
    }
  }
  if (quoted) throw new BackendError("shell", `${name}: line ${start}: a quoted field is not closed`);
  if (!blank) records.push({ line: start, fields: [...fields, field] });
  const [header, ...rows] = records;
  if (!header) throw new BackendError("shell", `${name}: no header row`);
  return rows.map((r) => {
    if (r.fields.length !== header.fields.length) {
      throw new BackendError(
        "shell",
        `${name}: line ${r.line}: ${r.fields.length} fields, but the header has ${header.fields.length} columns`,
      );
    }
    return header.fields.map((h, k): [string, unknown] => [h, r.fields[k]]);
  });
}

const text = (v: unknown) => (typeof v === "string" ? v : JSON.stringify(v));

function assertions(spec: RequestSpec, status: number | null): AssertionResult[] {
  return spec.assertions
    .filter((a) => a.enabled && a.expression === "res.status")
    .map((a) => {
      const expected = Number(a.value);
      const ops: Record<string, (x: number) => boolean> = {
        eq: (x) => x === expected,
        neq: (x) => x !== expected,
        gt: (x) => x > expected,
        lt: (x) => x < expected,
      };
      const op: ((x: number) => boolean) | undefined = Object.hasOwn(ops, a.operator) ? ops[a.operator] : undefined;
      const passed = status !== null && op !== undefined && op(status);
      return {
        expression: a.expression,
        operator: a.operator,
        expected: a.value,
        actual: status === null ? null : String(status),
        passed,
        ...(op ? {} : { error: `unknown operator ${a.operator}` }),
      };
    });
}

export function mockRunner(host?: MockRunnerHost): RunnerApi {
  let active: { cancelled: boolean } | null = null;
  const need = (): MockRunnerHost => {
    if (!host) throw new BackendError("shell", "the mock runner has no backend");
    return host;
  };
  const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

  return {
    async plan(folder) {
      return planOfTree(await need().tree(), folder);
    },
    async parseData(file) {
      return parseDataMock(file.name, file.data);
    },
    async run(plan, input, onEvent) {
      const h = need();
      if (active) throw new BackendError("shell", "a run is already in progress");
      const token = { cancelled: false };
      active = token;
      const started = Date.now();
      const summary = emptySummary();
      try {
        const count = plan.data.length > 0 ? plan.data.length : Math.max(1, plan.iterations);
        onEvent({ kind: "started", total: count * plan.requests.length });
        let first = true;
        let lastLimits: SendReport["rate_limit"];
        const respect = plan.throttle?.mode === "respect";
        const maxWait = Math.min(plan.throttle?.max_wait_ms ?? 60_000, 600_000);
        outer: for (let iteration = 0; iteration < count; iteration++) {
          const row = plan.data[iteration] ?? [];
          for (const path of plan.requests) {
            if (!first && plan.delay_ms > 0) {
              for (let left = plan.delay_ms; left > 0; left -= 50) {
                if (token.cancelled) break;
                await sleep(Math.min(50, left));
              }
            }
            if (token.cancelled) {
              summary.stopped = true;
              break outer;
            }
            // Wave 18: hold the request back for the host's limits (the core's throttle).
            const wait = respect ? mockWaitMs(lastLimits) : 0;
            if (wait > maxWait) {
              summary.requests++;
              summary.failed++;
              summary.stopped = true;
              const report: SendReport = {
                version: 1,
                request_path: path,
                environment: input.environment,
                request: null,
                response: null,
                variables: [],
                unresolved: [],
                error: { kind: "prepare", message: `not sent: rate limit: ${Math.ceil(wait / 1000)} s, longer than the ${Math.ceil(maxWait / 1000)} s the run may wait (--max-wait); the run stops` },
              };
              onEvent({ kind: "step", step: { iteration, request_path: path, request_name: path, report, passed: false, skipped: false } });
              break outer;
            }
            if (wait > 0) {
              onEvent({
                kind: "waiting",
                request_path: path,
                until: Date.now() + wait,
                reason: `rate limit: 0 left, resets in ${Math.ceil(wait / 1000)} s`,
              });
              for (let left = wait; left > 0 && !token.cancelled; left -= 50) await sleep(Math.min(50, left));
              if (token.cancelled) {
                summary.stopped = true;
                break outer;
              }
            }
            first = false;
            onEvent({ kind: "request", iteration, request_path: path });
            const vars: [string, string][] = [...row.map(([n, v]): [string, string] => [n, text(v)]), ...input.vars];
            let spec: RequestSpec | null = null;
            try {
              spec = await h.getRequest(path);
            } catch {
              spec = null;
            }
            const result = spec ? await h.send(path, { environment: input.environment, vars }) : null;
            const report: SendReport = result?.report ?? {
              version: 1,
              request_path: path,
              environment: input.environment,
              request: null,
              response: null,
              variables: [],
              unresolved: [],
              error: { kind: "not-found", message: path },
            };
            if (result?.response_id != null) await h.releaseResponse(result.response_id);
            const status = report.response?.status ?? null;
            if (report.response && report.response.body.data.length > VIEW_BODY_LIMIT) {
              report.response = {
                ...report.response,
                body: { ...report.response.body, data: report.response.body.data.slice(0, VIEW_BODY_LIMIT) },
                truncated: true,
              };
            }
            if (report.error === null && status !== null) {
              report.tests = [
                {
                  name: "status is 2xx",
                  passed: status >= 200 && status < 300,
                  origin: "request",
                  ...(status >= 200 && status < 300 ? {} : { error: `expected 2xx, got ${status}` }),
                },
              ];
              const checked = spec ? assertions(spec, status) : [];
              if (checked.length > 0) report.assertions = checked;
            }
            const passed =
              report.error === null &&
              (report.tests ?? []).every((t) => t.passed) &&
              (report.assertions ?? []).every((a) => a.passed);
            const step: RunStep = {
              iteration,
              request_path: path,
              request_name: spec?.name ?? path,
              report,
              passed,
              skipped: false,
            };
            summary.requests++;
            if (passed) summary.passed++;
            else summary.failed++;
            for (const t of report.tests ?? []) t.passed ? summary.tests_passed++ : summary.tests_failed++;
            for (const a of report.assertions ?? []) a.passed ? summary.assertions_passed++ : summary.assertions_failed++;
            lastLimits = report.rate_limit;
            onEvent({ kind: "step", step });
            if (plan.bail && !passed) {
              summary.stopped = true;
              break outer;
            }
          }
        }
      } finally {
        active = null;
      }
      summary.duration_ms = Date.now() - started;
      onEvent({ kind: "finished", summary });
      return summary;
    },
    async cancel() {
      if (active) active.cancelled = true;
    },
  };
}
