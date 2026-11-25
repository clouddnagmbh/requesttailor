// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ViewsApi` of the mock backend (wave 18, strand G): a small mirror of `rt_app::views` for UI
// tests (`?backend=mock`). The file `<request>.views.json` is staged through the mock's
// companion files with the same bytes the core writes (two-space JSON, fixed key order); the
// JSONPath is the common subset (`$`, `.name`, `['name']`, `[n]`, `[*]`, `.*`). The core is the
// reference; this only has to behave the same way for the screens.

import { BackendError, type CompanionApi, type ViewsApi } from "./backend";
import type { RequestSpec, ResponseView, ViewResult } from "./types";

/** What the mock views use of the mock backend. */
export interface MockViewsHost {
  companions: CompanionApi;
  getRequest(path: string): Promise<RequestSpec>;
  /** The text of a kept response; `null` for none or binary. */
  responseText(id: number): string | null;
}

const ROWS_LIMIT = 5000;
const JQ_MISSING = "jq is not available in this build";

export function companionPath(request: string): string {
  const dot = request.lastIndexOf(".");
  const slash = request.lastIndexOf("/");
  return `${dot > slash ? request.slice(0, dot) : request}.views.json`;
}

interface File {
  version: number;
  views: ResponseView[];
}

/** The file text: fixed key order, two-space indent, final newline (`render_file`). */
export function renderFile(views: ResponseView[]): string {
  const ordered = views.map((v) => ({
    name: v.name,
    language: v.language,
    expression: v.expression,
    display: v.display,
    ...(v.columns && v.columns.length > 0 ? { columns: v.columns } : {}),
  }));
  return `${JSON.stringify({ version: 1, views: ordered }, null, 2)}\n`;
}

type Step = { kind: "key"; key: string } | { kind: "index"; index: number } | { kind: "all" };

/** The JSONPath subset; throws `BackendError("filter", …)` like the core's parser does. */
export function parsePath(expression: string): Step[] {
  const bad = (why: string) => new BackendError("filter", `invalid JSONPath expression: ${why}`);
  if (!expression.startsWith("$")) throw bad("a path starts with $");
  const steps: Step[] = [];
  let i = 1;
  while (i < expression.length) {
    const c = expression[i];
    if (c === ".") {
      const m = /^\.(\*|[A-Za-z_][A-Za-z0-9_-]*)/.exec(expression.slice(i));
      if (!m) throw bad(`unexpected text at ${i}`);
      steps.push(m[1] === "*" ? { kind: "all" } : { kind: "key", key: m[1] });
      i += m[0].length;
    } else if (c === "[") {
      const m = /^\[(?:(\*)|(-?\d+)|'([^']*)'|"([^"]*)")\]/.exec(expression.slice(i));
      if (!m) throw bad(`unexpected text at ${i}`);
      if (m[1]) steps.push({ kind: "all" });
      else if (m[2] !== undefined) steps.push({ kind: "index", index: Number(m[2]) });
      else steps.push({ kind: "key", key: m[3] ?? m[4] ?? "" });
      i += m[0].length;
    } else {
      throw bad(`unexpected text at ${i}`);
    }
  }
  return steps;
}

export function query(root: unknown, steps: Step[]): unknown[] {
  let nodes: unknown[] = [root];
  for (const step of steps) {
    const next: unknown[] = [];
    for (const n of nodes) {
      if (step.kind === "key") {
        if (n && typeof n === "object" && !Array.isArray(n) && step.key in n) next.push((n as Record<string, unknown>)[step.key]);
      } else if (step.kind === "index") {
        if (Array.isArray(n)) {
          const at = step.index < 0 ? n.length + step.index : step.index;
          if (at >= 0 && at < n.length) next.push(n[at]);
        }
      } else if (Array.isArray(n)) next.push(...n);
      else if (n && typeof n === "object") next.push(...Object.values(n));
    }
    nodes = next;
  }
  return nodes;
}

function notice(display: ViewResult["display"], text: string): ViewResult {
  return { display, values: [], columns: [], rows: [], notice: text };
}

export function evaluate(text: string | null, view: ResponseView): ViewResult {
  if (view.language === "jq") throw new BackendError("unsupported", JQ_MISSING);
  const steps = parsePath(view.expression);
  let value: unknown;
  try {
    value = JSON.parse(text ?? "");
  } catch {
    return notice(view.display, "The body is not JSON.");
  }
  const matches = query(value, steps);
  if (matches.length === 0) return notice(view.display, "The expression matched nothing.");
  if (view.display === "table") {
    const items = matches.length === 1 && Array.isArray(matches[0]) ? (matches[0] as unknown[]) : matches;
    if (items.length > 0 && items.every((x) => x && typeof x === "object" && !Array.isArray(x))) {
      const objects = items as Record<string, unknown>[];
      const columns = (view.columns ?? []).length > 0 ? [...(view.columns ?? [])] : [...new Set(objects.slice(0, 20).flatMap((o) => Object.keys(o)))].slice(0, 50);
      const rows = objects.slice(0, ROWS_LIMIT).map((o) => columns.map((c) => o[c] ?? null));
      return {
        display: "table",
        values: [],
        columns,
        rows,
        notice: rows.length < objects.length ? `Showing the first ${rows.length} of ${objects.length} rows.` : null,
      };
    }
    return { display: "json", values: matches, columns: [], rows: [], notice: "A table needs an array of objects; shown as JSON." };
  }
  return { display: view.display, values: matches, columns: [], rows: [], notice: null };
}

export function mockViews(host: MockViewsHost): ViewsApi {
  async function read(path: string): Promise<ResponseView[]> {
    await host.getRequest(path);
    const text = await host.companions.read(companionPath(path));
    if (text === null) return [];
    try {
      return (JSON.parse(text) as File).views;
    } catch (e) {
      throw new BackendError("format", `saved views: ${e instanceof Error ? e.message : String(e)}`);
    }
  }
  async function write(path: string, views: ResponseView[]): Promise<ResponseView[]> {
    await host.companions.stage(companionPath(path), renderFile(views));
    return views;
  }
  return {
    list: (path) => read(path),
    async save(path, view) {
      const name = view.name.trim();
      // eslint-disable-next-line no-control-regex
      if (name === "" || [...name].length > 80 || /[\u0000-\u001f\u007f]/.test(name)) {
        throw new BackendError("filter", "a view needs a name of 1 to 80 characters");
      }
      if (view.expression.trim() === "") throw new BackendError("filter", "a view needs an expression");
      if (view.language === "jq") throw new BackendError("unsupported", JQ_MISSING);
      parsePath(view.expression);
      const kept: ResponseView = {
        ...view,
        name,
        columns: view.display === "table" ? (view.columns ?? []).filter((c) => c !== "") : [],
      };
      const all = await read(path);
      const at = all.findIndex((v) => v.name === name);
      if (at >= 0) all[at] = kept;
      else all.push(kept);
      return write(path, all);
    },
    async remove(path, name) {
      return write(
        path,
        (await read(path)).filter((v) => v.name !== name),
      );
    },
    async apply(_path, responseId, view) {
      return evaluate(host.responseText(responseId), view);
    },
    async languages() {
      return ["jsonpath"];
    },
  };
}
