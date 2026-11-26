// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// What RequestTailor tells the Git mock (`@tailor-kit/git-ui/mock`): a file is a request spec
// (shown as YAML, merged field by field), and the fixtures are the demo collection: the
// scenarios of the Git panel, the files next to the requests, the collection of a changed file.

import {
  runScenario,
  type MockContent,
  type MockExtraFile,
  type MockFileMerge,
  type MockFixtures,
  type MockScenarioEdit,
  type MockScenarioEdits,
} from "@tailor-kit/git-ui/mock";

import { DEMO_ID } from "./mock-collections";
import type { RequestSpec } from "./types";

const clone = <T>(v: T): T => structuredClone(v);
const json = (v: unknown) => JSON.stringify(v ?? null);

const yamlScalar = (v: unknown): string => {
  if (v === null || v === undefined) return "null";
  if (typeof v !== "string") return String(v);
  return v === "" || /^[\s]|[\s]$|[:#{}[\],&*!|>'"%@`]|^(true|false|null|-?\d[\d.]*)$|^- /.test(v) || v.includes("\n")
    ? JSON.stringify(v)
    : v;
};

/** Minimal YAML writer for plain data (objects, arrays, scalars), two-space indent. */
function yamlLines(v: unknown, indent: string): string[] {
  const empty = (x: unknown) => (Array.isArray(x) ? x.length === 0 : typeof x === "object" && x !== null && Object.keys(x).length === 0);
  if (Array.isArray(v)) {
    return v.flatMap((item) => {
      if (typeof item !== "object" || item === null) return [`${indent}- ${yamlScalar(item)}`];
      const [first, ...rest] = yamlLines(item, `${indent}  `);
      return [`${indent}- ${first.slice(indent.length + 2)}`, ...rest];
    });
  }
  return Object.entries(v as Record<string, unknown>)
    .filter(([, x]) => x !== undefined)
    .flatMap(([k, x]) => {
      if (typeof x === "string" && x.includes("\n")) {
        // Block scalar, so a multi-line body diffs line by line.
        return [`${indent}${k}: |${x.endsWith("\n") ? "" : "-"}`, ...x.replace(/\n$/, "").split("\n").map((l) => (l ? `${indent}  ${l}` : ""))];
      }
      if (typeof x !== "object" || x === null) return [`${indent}${k}: ${yamlScalar(x)}`];
      if (empty(x)) return [`${indent}${k}: ${Array.isArray(x) ? "[]" : "{}"}`];
      return [`${indent}${k}:`, ...yamlLines(x, `${indent}  `)];
    });
}

/** A request file's text in the mock: the spec as readable YAML, like an OpenCollection
 *  request file (`info`, `http`, then what is set), so the Git view shows an ordinary diff. */
export function renderSpec(spec: RequestSpec): string {
  const { name, seq, method, url, headers, params, body, auth, vars, settings, scripts, assertions, actions, protocol } = spec;
  const doc: Record<string, unknown> = {
    info: { name, type: protocol?.type ?? "http", seq },
    http: { method, url, headers, params, body, auth },
    vars,
    settings,
    scripts,
    assertions,
    actions,
    protocol,
  };
  for (const key of ["vars", "scripts", "assertions", "actions", "protocol"]) {
    const x = doc[key];
    if (x === undefined || (Array.isArray(x) && x.length === 0)) delete doc[key];
  }
  return `${yamlLines(doc, "").join("\n")}\n`;
}

const sameRendered = (a: RequestSpec | undefined, b: RequestSpec | undefined) =>
  a === b || (a !== undefined && b !== undefined && renderSpec(a) === renderSpec(b));

/** One file of a three-way merge, field by field for requests. */
function mergeFile(path: string, b: RequestSpec | undefined, o: RequestSpec | undefined, t: RequestSpec | undefined): MockFileMerge<RequestSpec> | null {
  const same = (x: RequestSpec | undefined, y: RequestSpec | undefined) => json(x) === json(y);
  if (same(o, t) || same(b, t)) return null;
  if (same(b, o)) return { path, merged: t, clean: 1, conflicts: [] };
  if (!o || !t || !b) {
    return { path, merged: o, clean: 0, conflicts: [!o ? "deleted here, changed there" : !t ? "changed here, deleted there" : "added on both sides"] };
  }
  const merged: Record<string, unknown> = { ...o };
  let clean = 0;
  const conflicts: string[] = [];
  const bo = b as unknown as Record<string, unknown>;
  const oo = o as unknown as Record<string, unknown>;
  const to = t as unknown as Record<string, unknown>;
  for (const key of new Set([...Object.keys(bo), ...Object.keys(oo), ...Object.keys(to)])) {
    const [bv, ov, tv] = [json(bo[key]), json(oo[key]), json(to[key])];
    if (ov === tv || bv === tv) continue;
    if (bv === ov) {
      merged[key] = clone(to[key]);
      clean++;
    } else conflicts.push(key);
  }
  return { path, merged: conflicts.length ? o : (merged as unknown as RequestSpec), clean, conflicts };
}

/** A request spec is the file: YAML for the views, field by field for merges. */
export const specContent: MockContent<RequestSpec> = {
  render: renderSpec,
  same: sameRendered,
  identical: (a, b) => json(a) === json(b),
  merge: mergeFile,
};

function blank(name: string): RequestSpec {
  return {
    name,
    seq: null,
    method: "GET",
    url: "",
    headers: [],
    params: [],
    body: { type: "none" },
    auth: { type: "inherit" },
    vars: [],
    settings: { timeout_ms: null, follow_redirects: null, max_redirects: null, encode_url: null },
    scripts: [],
    assertions: [],
    actions: [],
  };
}

/** 24 × 16 px blue, and the same with an orange square (valid PNGs). */
export const LOGO_OLD =
  "iVBORw0KGgoAAAANSUhEUgAAABgAAAAQCAIAAACDRijCAAAAGklEQVR42mPQiDpBFcQwatCoQaMGjRpEGQIAjqjvEGSqPNsAAAAASUVORK5CYII=";
export const LOGO_NEW =
  "iVBORw0KGgoAAAANSUhEUgAAABgAAAAQCAIAAACDRijCAAAAIElEQVR42mPQiDpBFcQwogz60CNCEI0aNGrQCM4i5CEAArsJX6x4MK4AAAAASUVORK5CYII=";

const LFS = (oid: string, size: number) => `version https://git-lfs.github.com/spec/v1\noid sha256:${oid}\nsize ${size}\n`;

const COMMITTED: Record<string, MockExtraFile> = {
  "/.gitignore": { text: "*.log\n" },
  "/README.md": { text: "# Demo API\n\nRequests for the demo service.\nSee the docs folder for the guides.\n" },
  "/docs/guide.md": { text: "# Guide\n\nSend a request with Mod+Enter.\n" },
  "/assets/logo.png": { base64: LOGO_OLD, media: "image/png" },
  "/assets/model.glb": { text: LFS("1f2e3d4c5b6a79881f2e3d4c5b6a79881f2e3d4c5b6a79881f2e3d4c5b6a7988", 4_194_304) },
  "/data/blob.bin": { binary: 2048 },
  "/legacy/old.txt": { text: "Replaced by docs/guide.md.\n" },
};

/** What the working tree holds instead: `null` deletes the file. */
const WORKING: Record<string, MockExtraFile | null> = {
  "/README.md": { text: "# Demo API\n\nRequests for the demo service.\nSee the guides folder for the walkthroughs.\n" },
  "/assets/logo.png": { base64: LOGO_NEW, media: "image/png" },
  "/assets/model.glb": { text: LFS("9a8b7c6d5e4f30219a8b7c6d5e4f30219a8b7c6d5e4f30219a8b7c6d5e4f3021", 5_242_880) },
  "/data/blob.bin": { binary: 4096 },
  "/legacy/old.txt": null,
  "/notes/todo.md": { text: "- document the retry policy\n- add a request for /health\n" },
  "/build.log": { text: "built in 3.2 s\n" },
};

const IGNORED = ["/build.log"];


/** `rt_app::git::tree_path` for the mock's request files. */
function collectionFile(path: string): boolean {
  const parts = path.split("/");
  const file = parts.pop() ?? "";
  return file.endsWith(".yml") && parts.every((p) => p !== "" && !p.startsWith(".") && p !== "node_modules");
}

const edit = (path: string, message: string, change: Partial<RequestSpec>): MockScenarioEdit<RequestSpec> => ({
  path,
  message,
  apply: (base) => ({ ...(base ?? blank(path)), ...change }),
});

const EDITS: MockScenarioEdits<RequestSpec> = {
  conflictRemote: { path: "health.yml", message: "Remote change to health", apply: (base) => ({ ...(base ?? blank("Health")), url: "{{baseUrl}}/health/remote" }) },
  conflictLocal: { path: "health.yml", message: "Local change to health", apply: (base) => ({ ...(base ?? blank("Health")), url: "{{baseUrl}}/health/local" }) },
  ahead: edit("health.yml", "Use the new health URL", { url: "{{baseUrl}}/healthz" }),
  remoteA: edit("health.yml", "Health check v2", { url: "{{baseUrl}}/health/v2" }),
  remoteB: edit("users/list-users.yml", "Page size for users", { url: "{{baseUrl}}/users?page=1&size=20" }),
  branchA: edit("users/create-user.yml", "Send JSON when creating users", { headers: [{ name: "Content-Type", value: "application/json", enabled: true }] }),
  branchB: edit("users/list-users.yml", "Bearer token for listing users", { auth: { type: "bearer", token: "{{token}}" } }),
  branchC: edit("users/create-user.yml", "Move user creation to v2", { url: "{{baseUrl}}/v2/users" }),
  mainA: edit("users/list-users.yml", "Bigger pages for users", { url: "{{baseUrl}}/users?page=1&size=50" }),
  mainB: edit("demo/large.yml", "Clearer name for the large response", { name: "Large response (100 MB)" }),
  workModified: { path: "health.yml", apply: (base) => ({ ...(base ?? blank("health.yml")), method: "HEAD" }) },
  workAdded: { path: "users/delete-user.yml", file: { ...blank("Delete user"), method: "DELETE", url: "{{baseUrl}}/users/{{id}}" } },
  workDeleted: { path: "demo/blocked.yml" },
};

export const specFixtures: MockFixtures<RequestSpec> = {
  scope: "collection",
  fileNoun: "request file",
  refNamespace: "requesttailor",
  author: "RequestTailor",
  semantic: (path) => path.endsWith(".yml"),
  repository: "mock://demo",
  initialMessage: "Initial collection",
  syntheticFirst: "Start the API collection",
  initialFiles: () => new Map([["health.yml", { ...blank("Health"), url: "{{baseUrl}}/health" }]]),
  scenario: (core, name, first) => runScenario(core, name, first, EDITS),
  extraFiles: { committed: COMMITTED, working: WORKING, ignored: IGNORED },
  tag: (path, from) => (from === "working" || collectionFile(path) ? { collection: DEMO_ID } : {}),
};
