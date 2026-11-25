// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `NotebookApi` of the mock backend (wave 19, strand W): a small mirror of `rt_app::notebook`
// for UI tests (`?backend=mock`). The mock keeps its notebooks in memory (staged, like the real
// ones until save) and plays a tiny API: `/login` answers `{access_token, user: {id, email}}`,
// `/orders` answers `{items: [{id, total}, ...]}`, anything else `{ok: true}`. The rules around
// it are the core's: blocks run in order and pass `set:` variables on (a credential-looking
// name is carried but shown as `***`), a writing block needs `allow_writes` and is never sent
// against a production environment, nothing of a run is stored.

import { BackendError, type NotebookApi } from "./backend";
import { mockEffect } from "./mock-agents";
import type {
  AgentPolicy,
  BlockResult,
  BlockView,
  Notebook,
  NotebookBlock,
  NotebookEntry,
  NotebookRun,
  RequestSpec,
  SendReport,
} from "./types";

export interface MockNotebookHost {
  getRequest(path: string): Promise<RequestSpec>;
  policy(): Promise<AgentPolicy>;
  environments(): Promise<{ name: string }[]>;
}

const MASK = "***";
const SUFFIX = ".notebook.md";
const looksLikeProduction = (name: string) => /prod|prd|\blive\b/i.test(name);
const credential = (name: string) => /token|secret|pass|key|auth|credential|session/i.test(name);

export const DEMO_NOTEBOOK_PATH = `orders${SUFFIX}`;

/** The notebook the mock starts with. */
export function demoNotebook(): Notebook {
  return {
    title: "Orders walkthrough",
    blocks: [
      {
        kind: "text",
        markdown:
          "# Orders walkthrough\n\nLog in, then read the orders of that user. Variables travel from block to block.\n\n",
      },
      {
        kind: "request",
        id: "login",
        embedded: demoSpec("GET", "{{base}}/login"),
        set: [
          ["token", "$.access_token"],
          ["userId", "$.user.id"],
        ],
        show: [{ kind: "response" }],
      },
      { kind: "text", markdown: "\nNow the orders, with the token from the block above:\n\n" },
      {
        kind: "request",
        id: "list",
        embedded: {
          ...demoSpec("GET", "{{base}}/orders"),
          headers: [{ name: "Authorization", value: "Bearer {{token}}", enabled: true }],
        },
        show: [{ kind: "table", path: "$.items[*]" }, { kind: "chart", path: "$.items[*].total" }],
      },
      { kind: "text", markdown: "\nA writing block never runs without permission:\n\n" },
      {
        kind: "request",
        id: "create",
        embedded: demoSpec("POST", "{{base}}/orders"),
        show: [{ kind: "response" }],
      },
    ],
  };
}

function demoSpec(method: string, url: string): RequestSpec {
  return {
    name: "",
    method,
    url,
    headers: [],
    params: [],
    body: { kind: "none" },
    auth: { type: "inherit" },
    vars: [],
    settings: {},
    scripts: [],
    assertions: [],
    actions: [],
  } as unknown as RequestSpec;
}

/** A tiny JSONPath: `$.a.b`, `$.a[*].b`, `$.a[*]`. Enough for the mock's fake API. */
function query(value: unknown, path: string): unknown[] {
  if (!path.startsWith("$")) return [];
  let nodes: unknown[] = [value];
  for (const part of path.slice(1).split(".").filter(Boolean)) {
    const m = /^([^[\]]*)(\[\*\])?$/.exec(part);
    if (!m) return [];
    const key = m[1];
    nodes = nodes.flatMap((n) => {
      const child = key ? (n as Record<string, unknown> | null)?.[key] : n;
      if (child === undefined) return [];
      return m[2] ? (Array.isArray(child) ? child : []) : [child];
    });
  }
  return nodes;
}

const text = (v: unknown) => (typeof v === "string" ? v : JSON.stringify(v));

function fakeAnswer(url: string, auth: string): unknown {
  const path = url.replace(/^[a-z]+:\/\/[^/]*/i, "").split("?")[0];
  if (path === "/login") return { access_token: "mock-token-1234", user: { id: 42, email: "someone@example.test" } };
  if (path === "/orders") return { items: [{ id: 1, total: 10.5 }, { id: 2, total: 4 }, { id: 3, total: 7.25 }], auth };
  return { ok: true };
}

function report(spec: RequestSpec, environment: string | null, vars: [string, string][], path: string): SendReport {
  const resolve = (s: string) =>
    s.replace(/\{\{\s*([\w.-]+)\s*\}\}/g, (all, n: string) =>
      n === "base" ? "http://api.test" : vars.find(([k]) => k === n)?.[1] ?? all,
    );
  const url = resolve(spec.url);
  const headers: [string, string][] = spec.headers
    .filter((h) => h.enabled)
    .map((h): [string, string] => [h.name, credential(h.name) ? MASK : resolve(h.value)]);
  const auth = spec.headers.find((h) => h.name.toLowerCase() === "authorization");
  const body = JSON.stringify(fakeAnswer(url, auth ? MASK : ""));
  return {
    version: 1,
    request_path: path,
    environment,
    request: { method: spec.method, url, headers, body_size: 0 },
    response: {
      status: 200,
      version: "HTTP/1.1",
      headers: [["content-type", "application/json"]],
      url,
      redirects: [],
      body: { encoding: "utf8", data: body },
      size: body.length,
      truncated: false,
      timing: { dns: 1, connect: 2, tls: null, ttfb: 9 },
      total: 12,
    },
    variables: [],
    unresolved: [],
    error: null,
  };
}

export function mockNotebook(host?: MockNotebookHost): NotebookApi {
  const store = new Map<string, Notebook>([[DEMO_NOTEBOOK_PATH, demoNotebook()]]);
  let busy = false;
  const count = (n: Notebook) => n.blocks.filter((b) => b.kind === "request").length;
  const get = (path: string) => {
    const n = store.get(path);
    if (!n) throw new BackendError("shell", `no notebook ${path}`);
    return n;
  };
  return {
    list: async (): Promise<NotebookEntry[]> =>
      [...store.entries()]
        .sort(([a], [b]) => a.localeCompare(b))
        .map(([path, n]) => ({ path, title: n.title, blocks: count(n) })),
    read: async (path) => structuredClone(get(path)),
    stage: async (path, notebook) => {
      if (!path.endsWith(SUFFIX)) throw new BackendError("shell", `${path} is not a notebook file (it ends in ${SUFFIX})`);
      store.set(path, structuredClone(notebook));
    },
    run: async (run: NotebookRun) => {
      if (busy) throw new BackendError("shell", "a notebook is already running");
      busy = true;
      try {
        const notebook = get(run.path);
        const environment = run.environment ?? null;
        const names = host ? (await host.environments()).map((e) => e.name) : [];
        if (environment && host && !names.includes(environment)) throw new BackendError("environment-not-found", environment);
        const policy = host ? await host.policy() : null;
        const entry = policy?.environments.find((e) => e.environment === environment);
        const production = environment ? (entry ? entry.production : looksLikeProduction(environment)) : false;
        const blocks = notebook.blocks.filter((b): b is Extract<NotebookBlock, { kind: "request" }> => b.kind === "request");
        if (run.block && !blocks.some((b) => b.id === run.block)) throw new BackendError("shell", `${run.path} has no block "${run.block}"`);
        const vars: [string, string][] = (run.vars ?? []).filter(([, v]) => v !== MASK);
        const results: BlockResult[] = [];
        for (const b of blocks) {
          if (run.block && run.block !== b.id) continue;
          let spec = b.embedded ?? null;
          if (!spec && b.ref && host) spec = await host.getRequest(b.ref).catch(() => null);
          if (!spec) {
            results.push({ block: b.id, skipped: `no request ${b.ref ?? ""} in the collection` });
            break;
          }
          if (mockEffect(spec) === "write") {
            if (production) {
              results.push({ block: b.id, skipped: "skipped (production)" });
              continue;
            }
            if (!run.allow_writes) {
              results.push({ block: b.id, skipped: "skipped (writes)" });
              continue;
            }
          }
          const rep = report(spec, environment, vars, b.ref ?? `${run.path}#${b.id}`);
          const json = JSON.parse(rep.response?.body.data ?? "null") as unknown;
          const result: BlockResult = { block: b.id, report: rep };
          const set: [string, string][] = [];
          const notes: string[] = [];
          for (const [name, path] of b.set ?? []) {
            const first = query(json, path)[0];
            if (first === undefined) {
              notes.push(`${name}: ${path} matched nothing; the variable is not set`);
              continue;
            }
            const value = text(first);
            vars.splice(0, vars.length, ...vars.filter(([n]) => n !== name), [name, value]);
            set.push([name, credential(name) ? MASK : value]);
          }
          if (set.length) result.set = set;
          if (notes.length) result.notes = notes;
          const views: BlockView[] = [];
          for (const s of b.show ?? []) {
            if (s.kind === "response") continue;
            const matches = query(json, s.path);
            if (s.kind === "table") {
              const rows = matches.filter((m): m is Record<string, unknown> => typeof m === "object" && m !== null);
              const columns = [...new Set(rows.flatMap((r) => Object.keys(r)))];
              views.push({ show: s, result: { display: "table", values: [], columns, rows: rows.map((r) => columns.map((c) => r[c])) } });
            } else {
              views.push({ show: s, result: { display: "json", values: matches } });
            }
          }
          if (views.length) result.views = views;
          results.push(result);
        }
        return { results };
      } finally {
        busy = false;
      }
    },
  };
}
