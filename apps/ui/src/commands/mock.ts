// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// In-memory backend for UI development and the UI strands' Playwright tests (`?backend=mock`).
// It mimics the real command layer's behaviour, not its file format: `save()` renders a
// simplified text, never real OpenCollection YAML. Special requests:
//   - `demo/large.yml`   responds with a virtual 100 MB body (lines generated on demand)
//   - `demo/blocked.yml` fails like a request the browser blocks (CORS), to show the
//                        companion offer
//   - a URL ending in `/login` answers `{"token": "mock-token-123", "user": {...}}` (wave 7)
//   - a URL ending in `/xml`, `/html`, `/css` or `/json-compact` answers a fixed body of
//     that type (pretty view tests, wave 8)
// Everything else answers like rt-testserver's `/echo`.
// Wave 7 (strand C): session variables go into the runtime layer, assertions and actions run
// in a tiny model (`mock-assert.ts`), a send keeps its `var_changes` for the session.
// `scriptOutcome` lets UI tests add script results (tests, logs, notes, a script error).
// Wave 14 (strand A): "Open folder" opens two collections ("Demo API" with the requests above,
// "Billing API"), `./mock-collections.ts`; the per-collection commands act on the active one.

import type {
  AuthApi,
  Backend,
  CollectionInfo,
  CompanionStatus,
  GitApi,
  ImportApi,
  PluginApi,
  RecentCollection,
  SecretsApi,
  SendInput,
  TransportApi,
  TransportMode,
} from "./backend";
import { BackendError } from "./backend";
import { mockOpenApi } from "./mock-openapi";
import { mockAuth } from "./mock-auth";
import { maskCookieHeader, maskSetCookie, mockCookies, setCookiesFor } from "./mock-cookies";
import { mockRunner } from "./mock-runner";
import { mockStreams } from "./mock-streams";
import { mockGrpc } from "./mock-grpc";
import { mockGraphQl } from "./mock-graphql";
import { mockAgents } from "./mock-agents";
import { mockMcp } from "./mock-mcp-client";
import { mockAi } from "./mock-ai";
// Wave 12: one file per strand.
import { mockDiffs } from "./mock-diffs";
import { mockMerges } from "./mock-merges";
import { mockCompare } from "./mock-compare";
import { mockTimeline } from "./mock-timeline";
import { mockReview } from "./mock-review";
import { mockContract } from "./mock-contract";
import { mockRescue } from "./mock-rescue";
import { mockCompanions } from "./mock-companions";
import { DEMO_ID, MockCollections, mockTree } from "./mock-collections";
import { mockFolderScripts } from "./mock-folder-scripts";
import { mockShare } from "./mock-share";
import { evaluateSnapshots, mockSnapshots } from "./mock-snapshots";
import { mockEnvCompare } from "./mock-envcompare";
import { mockInspect } from "./mock-inspect";
import { mockPaging } from "./mock-paging";
import { mockInbox } from "./mock-inbox";
import { mockLint } from "./mock-lint";
import { mockViews } from "./mock-views";
import { mockCatalogue } from "./mock-catalogue";
import { mockVariables } from "./mock-variables";
import type { MockWave12Host } from "./mock-wave12";
import { mockActions, mockEvaluate } from "./mock-assert";
import { mockScripts } from "./mock-scripts";
import { mockGit, type MockGit } from "./mock-git";
import { MockHistory, mockSearch } from "./mock-history";
import { mockImports } from "./mock-import";
import { mockPlugins, type MockPlugins } from "./mock-plugins";
import { MockRecent } from "./mock-recent";
import { mockRepositories, type MockRepositories } from "./mock-repositories";
import type {
  Environment,
  FileChange,
  LineSlice,
  Preview,
  RequestSpec,
  SearchHit,
  SendReport,
  StoredResponse,
  TreeNode,
  VarUse,
} from "./types";

const emptySettings = () => ({
  timeout_ms: null,
  follow_redirects: null,
  max_redirects: null,
  encode_url: null,
});

export function emptyRequest(name: string): RequestSpec {
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
    settings: emptySettings(),
    scripts: [],
    assertions: [],
    actions: [],
  };
}

function sampleRequests(): Map<string, RequestSpec> {
  const r = (name: string, method: string, url: string, extra: Partial<RequestSpec> = {}) => ({
    ...emptyRequest(name),
    method,
    url,
    ...extra,
  });
  return new Map<string, RequestSpec>([
    [
      "users/list-users.yml",
      r("List users", "GET", "{{baseUrl}}/users?page=1", {
        params: [{ name: "page", value: "1", kind: "query", enabled: true }],
        headers: [{ name: "Accept", value: "application/json", enabled: true }],
      }),
    ],
    [
      "users/create-user.yml",
      r("Create user", "POST", "{{baseUrl}}/users", {
        body: { type: "raw", kind: "json", data: '{\n  "name": "Ada"\n}' },
        auth: { type: "bearer", token: "{{token}}" },
      }),
    ],
    ["demo/large.yml", r("100 MB response", "GET", "{{baseUrl}}/bytes/104857600")],
    ["demo/blocked.yml", r("Blocked by the browser", "GET", "https://intranet.example/api")],
    ["health.yml", r("Health", "GET", "{{baseUrl}}/health")],
  ]);
}

const sampleEnvironments = (): Environment[] => [
  {
    name: "Local",
    client_certificates: [],
    external_secrets: null,
    variables: [
      { name: "baseUrl", value: "http://127.0.0.1:47811", enabled: true, secret: false },
      { name: "token", value: null, enabled: true, secret: true },
    ],
  },
  {
    name: "PRD",
    client_certificates: [],
    external_secrets: null,
    variables: [
      { name: "baseUrl", value: "https://api.example.com", enabled: true, secret: false },
      { name: "token", value: null, enabled: true, secret: true },
    ],
  },
];

/** Compact sample bodies by URL ending, for the pretty view (wave 8). */
function sampleBody(url: string): { body: string; contentType: string } | null {
  const end = /\/(xml|html|css|json-compact)(\?|$)/.exec(url)?.[1];
  switch (end) {
    case "css":
      return {
        contentType: "text/css; charset=utf-8",
        body: "body {\n  color: #333;\n  margin: 0;\n}\n\n@media (max-width: 600px) {\n  .nav {\n    display: none;\n  }\n}\n",
      };
    case "xml":
      return {
        contentType: "application/xml; charset=utf-8",
        body: '<?xml version="1.0"?><users count="2"><!-- page 1 --><user id="1"><name>Ada</name><tags><tag>admin</tag></tags></user><user id="2"><name>Grace</name><note><![CDATA[a < b]]></note></user></users>',
      };
    case "html":
      return {
        contentType: "text/html",
        body: "<!DOCTYPE html>\n<html>\n<head>\n<title>Mock</title>\n</head>\n<body>\n<p>Hello<br>world</p>\n<pre>  keep\n   me</pre>\n</body>\n</html>\n",
      };
    case "json-compact":
      return {
        contentType: "application/json",
        body: '{"id":12345678901234567890,"price":1.0,"items":[{"name":"a"},{"name":"b"}],"empty":{}}',
      };
    default:
      return null;
  }
}

/** Line `i` of the virtual large body: 63 characters plus `\n` = 64 bytes per line. */
const LARGE_SIZE = 100 * 1024 * 1024;
const LARGE_LINE_BYTES = 64;
const largeLine = (i: number) => `{"line": ${String(i).padStart(9, "0")}, "data": "${"x".repeat(32)}"}`;

interface Stored {
  lines: (start: number, count: number) => string[];
  lineCount: number;
  bytes: Uint8Array | null;
  text: string | null;
}

export class MockBackend implements Backend {
  readonly kind = "mock" as const;
  /** Every file "written" by `save`, for tests (`window.__rt.backend.files`); files of a
   *  collection in a subfolder under that folder (`billing/health.yml`). */
  readonly files = new Map<string, string>();
  /** Wave 14: the collections in the sidebar (`mock-collections.ts`). */
  readonly collections: MockCollections = new MockCollections({
    demo: () => ({ requests: sampleRequests(), envs: sampleEnvironments() }),
    empty: (name) => emptyRequest(name),
    files: this.files,
    extraDirty: () => [...this.mcp.dirty(), ...this.companions.dirty(), ...this.variables.dirty(), ...this.folderScripts.dirty()],
    saveExtra: () => {
      const changes: FileChange[] = [];
      for (const change of this.mcp.save()) {
        this.files.set(change.path, change.text);
        changes.push(change);
      }
      changes.push(...this.companions.save());
      for (const change of [...this.variables.save(), ...this.folderScripts.save()]) {
        this.files.set(change.path, change.text);
        changes.push(change);
      }
      return changes;
    },
  });
  /** Wave 15: repositories in the set (`mock-repositories.ts`). */
  readonly repositories: MockRepositories = mockRepositories({ collections: this.collections, git: () => this.git });

  // The active collection's data (throws like the shells when nothing is open).
  private get requests(): Map<string, RequestSpec> {
    return this.collections.current_().requests;
  }
  private get saved(): Map<string, RequestSpec> {
    return this.collections.current_().saved;
  }
  private get envs(): Environment[] {
    return this.collections.current_().envs;
  }
  private set envs(envs: Environment[]) {
    this.collections.current_().envs = envs;
  }
  private get envDirty(): Set<string> {
    return this.collections.current_().envDirty;
  }
  private get secretValues(): Map<string, string> {
    return this.collections.current_().secretValues;
  }
  private responses = new Map<number, Stored>();
  /** Wave 17: what a snapshot needs of a kept response (status and allowlisted headers). */
  private responseMeta = new Map<number, { status: number; headers: [string, string][] }>();
  private nextId = 1;
  private mode: TransportMode = "browser";

  /** `welcome`: ship a bundled plugin and ask the first-start question (wave 6). */
  constructor(options: { welcome?: boolean } = {}) {
    this.plugins = Object.assign(mockPlugins({ welcome: options.welcome }), {
      requestOf: (path: string) => this.getRequest(path),
      newRequest: (name: string) => emptyRequest(name),
    });
  }

  // Wave 4 parts, one file per strand (./mock-*.ts).
  readonly git: GitApi = mockGit({
    requests: () => this.requests,
    saved: () => this.saved,
    dirty: () => this.dirty(),
    openCollection: () => this.openCollection(),
  });
  readonly openapi = mockOpenApi({
    getRequest: (p) => this.getRequest(p),
    createRequest: (folder, name) => this.createRequest(folder, name),
    updateRequest: (p, spec) => this.updateRequest(p, spec),
    environments: () => this.environments(),
    updateEnvironment: (env) => this.updateEnvironment(env),
    hasSecret: async (env, name) => this.secretValues.has(`${env}/${name}`),
    folderNames: async () => [...new Set([...this.requests.keys()].flatMap((p) => (p.includes("/") ? [p.split("/")[0]!] : [])))],
    response: (id) => {
      const s = this.responses.get(id);
      return s ? { status: 200, text: s.text, contentType: "application/json" } : null;
    },
  });
  private readonly baseImports: ImportApi = mockImports(
    () => this.openCollection(),
    () => this.plugins,
    async (wanted, names) => {
      const root = await this.tree();
      const children = root.kind === "folder" ? root.children : [];
      const taken = new Set(children.flatMap((c) => (c.kind === "folder" ? [c.name.toLowerCase()] : [])));
      let name = wanted;
      for (let i = 2; taken.has(name.toLowerCase()); i++) name = `${wanted} ${i}`;
      const folder = await this.createFolder(name);
      for (const n of names) await this.createRequest(folder, n);
      return { folder, name };
    },
  );
  /** Plans of the OpenAPI wizard (`mock-openapi.ts`) are written by it, all others as before. */
  readonly imports: ImportApi = {
    ...this.baseImports,
    write: async (id) => {
      const into = this.openapi.writeInto(id, null);
      if (!into) return this.baseImports.write(id);
      const added = await into;
      const info = (await this.openCollection())!;
      return { ...info, location: `mock://${added.name}` };
    },
    writeInto: async (id, name) => (await this.openapi.writeInto(id, name)) ?? this.baseImports.writeInto(id, name),
    discard: async (id) => {
      if (!this.openapi.discardPlan(id)) await this.baseImports.discard(id);
    },
  };
  readonly history = new MockHistory();
  readonly recent = new MockRecent();
  // Wave 5 parts, one file per strand.
  readonly plugins: MockPlugins;
  readonly auth: AuthApi = mockAuth();
  readonly cookies = mockCookies();
  readonly scripts = mockScripts();
  readonly runner = mockRunner({
    tree: () => this.tree(),
    getRequest: (path) => this.getRequest(path),
    send: (path, input) => this.sendNow(path, input),
    releaseResponse: (id) => this.releaseResponse(id),
  });
  /** Wave 7: what "scripts" add to a send's report, for UI tests (the mock runs no JavaScript).
   *  A report with `error` set stops before the response, like `SendError::Script`. */
  scriptOutcome: ((path: string) => Partial<SendReport> | null) | null = null;
  /** Wave 8 (one file per strand). */
  readonly streams = mockStreams({ getRequest: (path) => this.getRequest(path) });
  readonly grpc = mockGrpc();
  readonly graphql = mockGraphQl();
  readonly agents = mockAgents({ getRequest: (path) => this.getRequest(path) });
  readonly mcp = mockMcp({
    lookup: (input, name) =>
      input.vars.find(([n]) => n === name)?.[1] ??
      this.envs
        .find((e) => e.name === input.environment)
        ?.variables.find((v) => v.name === name && v.enabled && !v.secret)?.value ??
      undefined,
  });
  readonly ai = mockAi();
  /** Wave 12 (one file per strand); `wave12` is what their parts read (./mock-wave12.ts). */
  private readonly wave12: MockWave12Host = {
    backend: this,
    git: () => this.git as MockGit,
    requests: () => this.requests,
    saved: () => this.saved,
    environments: () => this.envs,
    send: (path, input) => this.sendNow(path, input),
  };
  readonly diffs = mockDiffs(this.wave12);
  readonly merges = mockMerges(this.wave12);
  readonly compare = mockCompare(this.wave12);
  readonly timeline = mockTimeline(this.wave12);
  readonly review = mockReview(this.wave12);
  readonly contract = mockContract(this.wave12);
  readonly rescue = mockRescue(this.wave12);
  /** Wave 13: companion files for extensions. */
  readonly companions = mockCompanions(this.files, () => this.ensureOpen());
  /** Wave 14: variable completion; also the collection and folder variables of the send. */
  readonly variables = mockVariables({
    requests: () => this.requests,
    environments: () => this.envs,
    sessionVars: (environment) => this.scripts.sessionVars(environment),
    secretValues: (environment) =>
      [...this.secretValues].flatMap(([key, value]) => (key.startsWith(`${environment}/`) ? [value] : [])),
  });

  /** Wave 16: scripts of the collection and its folders. */
  readonly folderScripts = mockFolderScripts(() => this.requests);
  /** Wave 18: placeholders until the strands (`wave18-stubs.ts`). */
  readonly envCompare = mockEnvCompare();
  readonly inspect = mockInspect();
  readonly paging = mockPaging();
  readonly inbox = mockInbox();
  readonly lint = mockLint({
    requests: () => this.requests,
    environments: () => this.envs,
    variables: this.variables,
    name: () => this.collections.current_().info.name,
  });
  readonly views = mockViews();
  readonly catalogue = mockCatalogue();
  /** Wave 17 (strands B and C replace the placeholders in their own files). */
  readonly snapshots = mockSnapshots({
    companions: this.companions,
    getRequest: (p) => this.getRequest(p),
    updateRequest: (p, spec) => this.updateRequest(p, spec),
    requestPaths: () => [...this.requests.keys()],
    response: (id) => {
      const s = this.responses.get(id);
      const meta = this.responseMeta.get(id);
      return s?.text != null && meta ? { ...meta, body: s.text } : undefined;
    },
  });
  readonly share = mockShare({
    getRequest: (p) => this.getRequest(p),
    environments: () => this.environments(),
  });

  async openCollection(): Promise<CollectionInfo | null> {
    const info = this.collections.openDemo();
    this.recent.opened({ id: DEMO_ID, name: "demo", location: "mock://demo" });
    return info;
  }

  async recentCollections(): Promise<RecentCollection[]> {
    return this.recent.list();
  }

  async openRecent(id: string, _interactive: boolean): Promise<CollectionInfo | null> {
    return id === DEMO_ID ? this.openCollection() : null;
  }

  async search(query: string, limit: number): Promise<SearchHit[]> {
    this.ensureOpen();
    return mockSearch(this.requests, query, limit);
  }

  private ensureOpen() {
    if (!this.collections.isOpen) throw new BackendError("shell", "no collection open");
  }

  async tree(): Promise<TreeNode> {
    this.ensureOpen();
    const c = this.collections.current_();
    return mockTree(c.info.name, c.requests, c.folders);
  }

  async getRequest(path: string): Promise<RequestSpec> {
    this.ensureOpen();
    const spec = this.requests.get(path);
    if (!spec) throw new BackendError("request-not-found", path);
    return structuredClone(spec);
  }

  async createRequest(folder: string, name: string): Promise<string> {
    this.ensureOpen();
    const slug = name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "request";
    let path = `${folder ? folder + "/" : ""}${slug}.yml`;
    for (let n = 2; this.requests.has(path); n++) path = `${folder ? folder + "/" : ""}${slug}-${n}.yml`;
    this.requests.set(path, emptyRequest(name));
    return path;
  }

  /** Segments keep their names; the folder shows in the tree before it has requests. */
  async createFolder(path: string): Promise<string> {
    this.ensureOpen();
    const parts = path.split("/").filter((p) => p !== "");
    if (parts.some((p) => p === "." || p === ".." || p.includes("\\"))) throw new BackendError("folder-not-found", path);
    const folder = parts.join("/");
    if (folder) this.collections.current_().folders.add(folder);
    return folder;
  }

  async updateRequest(path: string, spec: RequestSpec): Promise<void> {
    this.ensureOpen();
    if (!this.requests.has(path)) throw new BackendError("request-not-found", path);
    this.requests.set(path, structuredClone(spec));
  }

  async dirty(): Promise<string[]> {
    if (!this.collections.isOpen) return [];
    return this.collections.dirtyOf(this.collections.current_(), true);
  }

  async save(): Promise<FileChange[]> {
    if (!this.collections.isOpen) return [];
    return this.collections.saveOf(this.collections.current_(), true);
  }

  async environments(): Promise<Environment[]> {
    return structuredClone(this.envs);
  }

  async updateEnvironment(env: Environment): Promise<void> {
    const clean = structuredClone(env);
    for (const v of clean.variables) if (v.secret) v.value = null;
    const i = this.envs.findIndex((e) => e.name === env.name);
    if (i >= 0) this.envs[i] = clean;
    else this.envs.push(clean);
    this.envDirty.add(env.name);
  }

  async deleteEnvironment(name: string): Promise<void> {
    if (!this.envs.some((e) => e.name === name)) throw new BackendError("environment-not-found", name);
    this.envs = this.envs.filter((e) => e.name !== name);
    this.envDirty.add(name);
  }

  async preview(path: string, input: SendInput): Promise<Preview> {
    const r = (await this.sendNow(path, input)).report;
    const spec = await this.getRequest(path);
    const env = this.envs.find((e) => e.name === input.environment);
    const session = await this.scripts.sessionVars(input.environment ?? "");
    const runtime = (name: string) =>
      input.vars.find(([n]) => n === name)?.[1] ?? [...session].reverse().find((c) => c.name === name)?.value ?? undefined;
    return {
      url: r.request?.url ?? null,
      headers: r.request?.headers ?? [],
      variables: r.variables.map((u) => ({
        ...u,
        value: u.secret
          ? null
          : u.source.layer === "runtime"
            ? (runtime(u.name) ?? null)
            : u.source.layer === "request"
              ? (spec.vars.find((v) => v.name === u.name)?.value ?? null)
              : u.source.layer === "folder"
                ? (this.variables.lookup(path, u.name, "folders")?.value ?? null)
                : u.source.layer === "collection"
                  ? (this.variables.lookup(path, u.name, "collection")?.value ?? null)
                  : (env?.variables.find((v) => v.name === u.name)?.value ?? null),
      })),
      unresolved: r.unresolved,
      error: null,
    };
  }

  async send(path: string, input: SendInput): Promise<StoredResponse> {
    const result = await this.sendNow(path, input);
    for (const change of result.report.var_changes ?? []) {
      await this.scripts.setSessionVar(input.environment ?? "", change);
    }
    const notes = this.plugins.notes();
    if (notes.length > 0) result.report.notes = [...(result.report.notes ?? []), ...notes];
    this.history.record(this.collections.current_().info.id, await this.getRequest(path), result);
    return result;
  }

  private async sendNow(path: string, input: SendInput): Promise<StoredResponse> {
    const spec = await this.getRequest(path);
    const env = this.envs.find((e) => e.name === input.environment) ?? null;
    // Session variables first in the runtime layer; the caller's own `vars` win.
    const session = (await this.scripts.sessionVars(input.environment ?? "")).flatMap((c): [string, string][] =>
      c.value === null ? [] : [[c.name, c.value]],
    );
    const runtimeVars = [...input.vars.slice().reverse(), ...session.reverse()];
    const used: VarUse[] = [];
    const unresolved: string[] = [];
    const secretValues: string[] = [];
    const resolve = (t: string) =>
      t.replace(/\{\{([^}]+)\}\}/g, (whole, name: string) => {
        const runtime = runtimeVars.find(([n]) => n === name);
        if (runtime) {
          used.push({ name, source: { layer: "runtime" }, secret: false });
          return runtime[1];
        }
        // Wave 14: request and folder variables above the environment.
        const own = spec.vars.find((x) => x.name === name && x.enabled && !x.secret && x.value !== null);
        if (own) {
          used.push({ name, source: { layer: "request" }, secret: false });
          return own.value!;
        }
        const inFolder = this.variables.lookup(path, name, "folders");
        if (inFolder) {
          used.push({ name, source: inFolder.source, secret: false });
          return inFolder.value;
        }
        const v = env?.variables.find((x) => x.name === name && x.enabled);
        if (v) {
          const value = v.secret
            ? this.secretValues.get(`${env!.name}/${name}`)
            : (v.value ?? undefined);
          if (value !== undefined) {
            used.push({ name, source: { layer: "environment", name: env!.name }, secret: v.secret });
            if (v.secret) secretValues.push(value);
            return value;
          }
        }
        const inCollection = this.variables.lookup(path, name, "collection");
        if (inCollection) {
          used.push({ name, source: inCollection.source, secret: false });
          return inCollection.value;
        }
        unresolved.push(name);
        return whole;
      });
    const url = resolve(spec.url);
    const headers: [string, string][] = spec.headers
      .filter((h) => h.enabled)
      .map((h) => [h.name, resolve(h.value)]);
    if (spec.auth.type === "bearer") headers.push(["Authorization", `Bearer ${resolve(spec.auth.token)}`]);
    // Cookie jar of the environment (strand D): sent like the core does, masked in the report.
    const jarCookie = this.cookies.header(input.environment ?? "", url);
    if (jarCookie !== null) headers.push(["Cookie", jarCookie]);
    const setCookie = setCookiesFor(url);
    const mask = (s: string) => secretValues.reduce((acc, v) => acc.split(v).join("***"), s);
    const reported = (n: string, v: string) => (n.toLowerCase() === "cookie" ? maskCookieHeader(v) : mask(v));

    const report: SendReport = {
      version: 1,
      request_path: path,
      environment: input.environment,
      request: { method: spec.method, url: mask(url), headers: headers.map(([n, v]) => [n, reported(n, v)]), body_size: 0 },
      response: null,
      variables: used,
      unresolved: [...new Set(unresolved)],
      error: null,
    };

    const outcome = this.scriptOutcome?.(path) ?? null;
    if (outcome?.error) {
      Object.assign(report, outcome, { request: null });
      return { report, response_id: null, size: 0, line_count: 0, is_text: false };
    }

    if (path === "demo/blocked.yml" && this.mode === "browser") {
      report.error = { kind: "transport", message: { kind: "blocked", message: "Failed to fetch" } };
      return { report, response_id: null, size: 0, line_count: 0, is_text: false };
    }

    const id = this.nextId++;
    let stored: Stored;
    let preview: string;
    let size: number;
    let contentType = "application/json";
    if (path === "demo/large.yml") {
      const lineCount = LARGE_SIZE / LARGE_LINE_BYTES;
      stored = {
        lineCount,
        lines: (start, count) =>
          Array.from({ length: Math.max(0, Math.min(count, lineCount - start)) }, (_, k) => largeLine(start + k)),
        bytes: null,
        text: null,
      };
      preview = stored.lines(0, 1024).join("\n");
      size = LARGE_SIZE;
    } else {
      const sample = sampleBody(url);
      if (sample) contentType = sample.contentType;
      const body = sample
        ? sample.body
        : /\/login(\?|$)/.test(url)
          ? JSON.stringify({ token: "mock-token-123", user: { id: 7, name: "Ada" } }, null, 2)
          : JSON.stringify(
              { method: spec.method, url: mask(url), headers: headers.map(([n, v]) => [n.toLowerCase(), reported(n, v)]) },
              null,
              2,
            );
      const lines = body.split("\n");
      stored = {
        lineCount: lines.length,
        lines: (start, count) => lines.slice(start, start + count),
        bytes: new TextEncoder().encode(body),
        text: body,
      };
      preview = body;
      size = stored.bytes!.length;
    }
    this.responses.set(id, stored);
    this.responseMeta.set(id, { status: 200, headers: [["content-type", contentType]] });
    this.cookies.store(input.environment ?? "", url, setCookie);
    report.response = {
      status: 200,
      version: "HTTP/1.1",
      headers: [["content-type", contentType], ...setCookie.map((v): [string, string] => ["set-cookie", maskSetCookie(v)])],
      url: mask(url),
      redirects: [],
      body: { encoding: "utf8", data: preview },
      size,
      truncated: false,
      timing: { dns: 1.2, connect: 2.5, tls: null, ttfb: 12.5 },
      total: 14.0,
    };
    if (stored.text !== null) {
      const lookup = (name: string) =>
        runtimeVars.find(([n]) => n === name)?.[1] ??
        env?.variables.find((v) => v.name === name && v.enabled && !v.secret)?.value ??
        undefined;
      const response = { status: 200, body: stored.text };
      const assertions = [
        ...mockEvaluate(spec.assertions.filter((a) => a.operator.trim() !== "matchesSnapshot"), response, lookup),
        ...(await evaluateSnapshots(
          spec.assertions,
          path,
          { status: 200, headers: [["content-type", contentType]], body: stored.text },
          (file) => this.companions.read(file),
        )),
      ];
      if (assertions.length > 0) report.assertions = assertions;
      const changes = mockActions(spec.actions, response);
      if (changes.length > 0) report.var_changes = changes;
    }
    if (outcome) {
      const { var_changes, ...rest } = outcome;
      Object.assign(report, rest);
      if (var_changes) report.var_changes = [...var_changes, ...(report.var_changes ?? [])];
    }
    return { report, response_id: id, size, line_count: stored.lineCount, is_text: true };
  }

  private stored(id: number): Stored {
    const s = this.responses.get(id);
    if (!s) throw new BackendError("response-not-found", String(id));
    return s;
  }

  async responseLines(id: number, start: number, count: number): Promise<LineSlice> {
    return { start, lines: this.stored(id).lines(start, count), cut: [] };
  }

  async responseBytes(id: number, offset: number, length: number): Promise<Uint8Array> {
    const s = this.stored(id);
    if (!s.bytes) return new TextEncoder().encode(s.lines(Math.floor(offset / LARGE_LINE_BYTES), Math.ceil(length / LARGE_LINE_BYTES)).join("\n")).slice(0, length);
    return s.bytes.slice(offset, offset + length);
  }

  async filterJson(id: number, expression: string): Promise<string> {
    const s = this.stored(id);
    if (s.text === null) throw new BackendError("filter", "body too large to filter");
    const value: unknown = JSON.parse(s.text);
    // Minimal JSONPath: `$` and `$.a.b` only.
    if (!/^\$(\.[A-Za-z_][A-Za-z0-9_]*)*$/.test(expression)) {
      throw new BackendError("filter", `mock supports only $.a.b paths: ${expression}`);
    }
    let cur: unknown = value;
    for (const key of expression.split(".").slice(1)) {
      cur = cur && typeof cur === "object" ? (cur as Record<string, unknown>)[key] : undefined;
    }
    return JSON.stringify(cur === undefined ? [] : [cur], null, 2);
  }

  async releaseResponse(id: number): Promise<void> {
    this.responses.delete(id);
  }

  readonly secrets: SecretsApi = {
    has: async (env, name) => this.secretValues.has(`${env}/${name}`),
    get: async (env, name) => this.secretValues.get(`${env}/${name}`) ?? null,
    set: async (env, name, value) => void this.secretValues.set(`${env}/${name}`, value),
    delete: async (env, name) => void this.secretValues.delete(`${env}/${name}`),
  };

  readonly transport: TransportApi = {
    mode: () => this.mode,
    companion: async (): Promise<CompanionStatus> => ({
      available: true,
      paired: true,
      url: "http://127.0.0.1:47812",
      howTo: "Run `rt agent` in a terminal.",
    }),
    useCompanion: async (enabled) => {
      this.mode = enabled ? "companion" : "browser";
    },
  };
}
