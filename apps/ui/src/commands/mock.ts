// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// In-memory backend for UI development and the UI strands' Playwright tests (`?backend=mock`).
// It mimics the real command layer's behaviour, not its file format: `save()` renders a
// simplified text, never real OpenCollection YAML. Special requests:
//   - `demo/large.yml`   responds with a virtual 100 MB body (lines generated on demand)
//   - `demo/blocked.yml` fails like a request the browser blocks (CORS), to show the
//                        companion offer
// Everything else answers like rt-testserver's `/echo`.

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
import { mockAuth } from "./mock-auth";
import { maskCookieHeader, maskSetCookie, mockCookies, setCookiesFor } from "./mock-cookies";
import { mockGit } from "./mock-git";
import { MockHistory, mockSearch } from "./mock-history";
import { mockImports } from "./mock-import";
import { mockPlugins } from "./mock-plugins";
import { MockRecent } from "./mock-recent";
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

  private requests = sampleRequests();
  private saved = new Map<string, RequestSpec>();
  private envs = sampleEnvironments();
  private secretValues = new Map<string, string>();
  private responses = new Map<number, Stored>();
  private nextId = 1;
  private mode: TransportMode = "browser";
  private opened = false;
  /** Every file "written" by `save`, for tests (`window.__rt.backend.files`). */
  readonly files = new Map<string, string>();

  constructor() {
    for (const [p, s] of this.requests) this.saved.set(p, structuredClone(s));
  }

  // Wave 4 parts, one file per strand (./mock-*.ts).
  readonly git: GitApi = mockGit({
    requests: () => this.requests,
    saved: () => this.saved,
    dirty: () => this.dirty(),
    openCollection: () => this.openCollection(),
  });
  readonly imports: ImportApi = mockImports(() => this.openCollection());
  readonly history = new MockHistory();
  readonly recent = new MockRecent();
  // Wave 5 parts, one file per strand.
  readonly plugins: PluginApi = mockPlugins();
  readonly auth: AuthApi = mockAuth();
  readonly cookies = mockCookies();

  async openCollection(): Promise<CollectionInfo | null> {
    this.opened = true;
    const info = { id: "mock-collection", name: "Demo API", location: "mock://demo" };
    this.recent.opened(info);
    return info;
  }

  async recentCollections(): Promise<RecentCollection[]> {
    return this.recent.list();
  }

  async openRecent(id: string, _interactive: boolean): Promise<CollectionInfo | null> {
    return id === "mock-collection" ? this.openCollection() : null;
  }

  async search(query: string, limit: number): Promise<SearchHit[]> {
    this.ensureOpen();
    return mockSearch(this.requests, query, limit);
  }

  private ensureOpen() {
    if (!this.opened) throw new BackendError("shell", "no collection open");
  }

  async tree(): Promise<TreeNode> {
    this.ensureOpen();
    const root: TreeNode = { kind: "folder", name: "Demo API", path: "", children: [] };
    const folders = new Map<string, TreeNode & { kind: "folder" }>();
    const paths = [...this.requests.keys()].sort();
    for (const path of paths) {
      const spec = this.requests.get(path)!;
      const parts = path.split("/");
      let parent = root as TreeNode & { kind: "folder" };
      for (let i = 0; i < parts.length - 1; i++) {
        const fp = parts.slice(0, i + 1).join("/");
        let f = folders.get(fp);
        if (!f) {
          f = { kind: "folder", name: parts[i], path: fp, children: [] };
          folders.set(fp, f);
          parent.children.push(f);
        }
        parent = f;
      }
      parent.children.push({ kind: "request", name: spec.name, path, method: spec.method });
    }
    return root;
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

  async updateRequest(path: string, spec: RequestSpec): Promise<void> {
    this.ensureOpen();
    if (!this.requests.has(path)) throw new BackendError("request-not-found", path);
    this.requests.set(path, structuredClone(spec));
  }

  async dirty(): Promise<string[]> {
    const requests = [...this.requests.keys()].filter(
      (p) => JSON.stringify(this.requests.get(p)) !== JSON.stringify(this.saved.get(p)),
    );
    return [...requests, ...[...this.envDirty].map((n) => `environments/${n}.yml`)];
  }

  async save(): Promise<FileChange[]> {
    const changes: FileChange[] = [];
    for (const path of (await this.dirty()).filter((p) => this.requests.has(p))) {
      const spec = this.requests.get(path)!;
      // Simplified rendering; secret values are never part of a request spec.
      const text = `# mock rendering, not OpenCollection\n${JSON.stringify(spec, null, 2)}\n`;
      this.files.set(path, text);
      this.saved.set(path, structuredClone(spec));
      changes.push({ path, text, deleted: false });
    }
    for (const name of this.envDirty) {
      const env = this.envs.find((e) => e.name === name);
      const path = `environments/${name}.yml`;
      // Secret values are never part of an Environment here (value: null).
      const text = env ? `# mock rendering\n${JSON.stringify(env, null, 2)}\n` : "";
      this.files.set(path, text);
      changes.push({ path, text, deleted: !env });
    }
    this.envDirty.clear();
    return changes;
  }

  async environments(): Promise<Environment[]> {
    return structuredClone(this.envs);
  }

  private envDirty = new Set<string>();

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
    const env = this.envs.find((e) => e.name === input.environment);
    return {
      url: r.request?.url ?? null,
      headers: r.request?.headers ?? [],
      variables: r.variables.map((u) => ({
        ...u,
        value: u.secret
          ? null
          : (input.vars.find(([n]) => n === u.name)?.[1] ??
            env?.variables.find((v) => v.name === u.name)?.value ??
            null),
      })),
      unresolved: r.unresolved,
      error: null,
    };
  }

  async send(path: string, input: SendInput): Promise<StoredResponse> {
    const result = await this.sendNow(path, input);
    this.history.record("mock-collection", await this.getRequest(path), result);
    return result;
  }

  private async sendNow(path: string, input: SendInput): Promise<StoredResponse> {
    const spec = await this.getRequest(path);
    const env = this.envs.find((e) => e.name === input.environment) ?? null;
    const used: VarUse[] = [];
    const unresolved: string[] = [];
    const secretValues: string[] = [];
    const resolve = (t: string) =>
      t.replace(/\{\{([^}]+)\}\}/g, (whole, name: string) => {
        const runtime = input.vars.find(([n]) => n === name);
        if (runtime) {
          used.push({ name, source: { layer: "runtime" }, secret: false });
          return runtime[1];
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

    if (path === "demo/blocked.yml" && this.mode === "browser") {
      report.error = { kind: "transport", message: { kind: "blocked", message: "Failed to fetch" } };
      return { report, response_id: null, size: 0, line_count: 0, is_text: false };
    }

    const id = this.nextId++;
    let stored: Stored;
    let preview: string;
    let size: number;
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
      const body = JSON.stringify(
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
    this.cookies.store(input.environment ?? "", url, setCookie);
    report.response = {
      status: 200,
      version: "HTTP/1.1",
      headers: [["content-type", "application/json"], ...setCookie.map((v): [string, string] => ["set-cookie", maskSetCookie(v)])],
      url: mask(url),
      redirects: [],
      body: { encoding: "utf8", data: preview },
      size,
      truncated: false,
      timing: { dns: 1.2, connect: 2.5, tls: null, ttfb: 12.5 },
      total: 14.0,
    };
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
