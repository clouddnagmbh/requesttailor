// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The collection set of the mock backend (wave 14, strand A). "Open folder" opens the root
// `mock://demo` with two collections: "Demo API" (the root itself, the sample requests every UI
// test knows) and "Billing API" (subfolder `billing`), which shares the path `health.yml` with
// Demo API. "Add collection…" adds the root `mock://payments` with "Payments API". Each
// collection keeps its own requests, environments and secret values; `MockBackend` serves the
// per-collection commands from the active one (`MockCollectionsHost.current`).

import type { CollectionInfo, CollectionSetApi } from "./backend";
import { BackendError } from "./backend";
import { mockSearch } from "./mock-history";
import type {
  CollectionChanges,
  CollectionEntry,
  CollectionSet,
  Environment,
  FileChange,
  NewCollection,
  RequestSpec,
  TreeNode,
} from "./types";

/** One collection of the mock set. */
export interface MockCollection {
  info: CollectionInfo;
  root: string;
  dir: string;
  repo: string | null;
  requests: Map<string, RequestSpec>;
  saved: Map<string, RequestSpec>;
  envs: Environment[];
  envDirty: Set<string>;
  secretValues: Map<string, string>;
  /** A new collection whose files are not written yet (`create`). */
  created: boolean;
}

interface MockRoot {
  id: string;
  name: string;
  location: string;
  collections: string[];
}

/** What the set needs from `MockBackend`. */
export interface MockCollectionsHost {
  /** The demo collection's fresh sample data. */
  demo(): Pick<MockCollection, "requests" | "envs">;
  empty(name: string): RequestSpec;
  /** Files "written" by saves (`MockBackend.files`). */
  files: Map<string, string>;
  /** Unsaved paths of the active collection that are not requests or environments (MCP
   *  tests, companion files), and saving them (they write `files` themselves). */
  extraDirty(): string[];
  saveExtra(): FileChange[];
}

export const DEMO_ID = "mock-collection";
export const BILLING_ID = "mock-collection/billing";
export const PAYMENTS_ID = "mock-payments";

const env = (name: string, baseUrl: string): Environment => ({
  name,
  client_certificates: [],
  external_secrets: null,
  variables: [
    { name: "baseUrl", value: baseUrl, enabled: true, secret: false },
    { name: "token", value: null, enabled: true, secret: true },
  ],
});

/** The tree of a mock collection (folders exist through their requests). */
export function mockTree(name: string, requests: ReadonlyMap<string, RequestSpec>): TreeNode {
  const root: TreeNode = { kind: "folder", name, path: "", children: [] };
  const folders = new Map<string, TreeNode & { kind: "folder" }>();
  for (const path of [...requests.keys()].sort()) {
    const spec = requests.get(path)!;
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

export class MockCollections implements CollectionSetApi {
  private roots: MockRoot[] = [];
  private readonly data = new Map<string, MockCollection>();
  private active: string | null = null;

  constructor(private readonly host: MockCollectionsHost) {
    // The demo folder's collections exist before "Open folder" (tests set secrets and Git
    // scenarios first); `isOpen` says whether the app opened them.
    this.seed();
  }

  /** The active collection (before anything is opened: Demo API); throws when the set was
   *  emptied. */
  current_(): MockCollection {
    const c = this.data.get(this.active ?? DEMO_ID);
    if (!c) throw new BackendError("shell", "no collection open");
    return c;
  }

  get isOpen(): boolean {
    return this.active !== null && this.data.has(this.active);
  }

  private collection(id: string): MockCollection {
    const c = this.data.get(id);
    if (!c) throw new BackendError("shell", `no collection ${id} in the sidebar`);
    return c;
  }

  private make(info: CollectionInfo, root: string, dir: string, requests: Map<string, RequestSpec>, envs: Environment[]): MockCollection {
    const saved = new Map<string, RequestSpec>();
    for (const [p, s] of requests) saved.set(p, structuredClone(s));
    return { info, root, dir, repo: "mock://demo", requests, saved, envs, envDirty: new Set(), secretValues: new Map(), created: false };
  }

  /** "Open folder": the demo root with its two collections, Demo API active. Opening it
   *  again keeps their state, as the real shells keep unsaved edits of a folder read again. */
  openDemo(): CollectionInfo {
    if (!this.data.has(DEMO_ID) || !this.data.has(BILLING_ID)) this.seed();
    for (const id of [...this.data.keys()]) if (id !== DEMO_ID && id !== BILLING_ID) this.data.delete(id);
    this.roots = [{ id: "mock://demo", name: "demo", location: "mock://demo", collections: [DEMO_ID, BILLING_ID] }];
    this.active = DEMO_ID;
    return this.data.get(DEMO_ID)!.info;
  }

  private seed(): void {
    this.data.clear();
    const demo = this.host.demo();
    this.data.set(DEMO_ID, this.make({ id: DEMO_ID, name: "Demo API", location: "mock://demo" }, "mock://demo", "", demo.requests, demo.envs));
    const r = (name: string, method: string, url: string): RequestSpec => ({ ...this.host.empty(name), method, url });
    this.data.set(
      BILLING_ID,
      this.make(
        { id: BILLING_ID, name: "Billing API", location: "mock://demo/billing" },
        "mock://demo",
        "billing",
        new Map([
          ["invoices/list-invoices.yml", r("List invoices", "GET", "{{baseUrl}}/invoices")],
          ["invoices/create-invoice.yml", r("Create invoice", "POST", "{{baseUrl}}/invoices")],
          ["health.yml", r("Billing health", "GET", "{{baseUrl}}/billing/health")],
        ]),
        [env("Staging", "http://127.0.0.1:47811/billing"), env("Local", "http://127.0.0.1:47811")],
      ),
    );
  }

  dirtyOf(c: MockCollection, extra: boolean): string[] {
    const requests = [...c.requests.keys()].filter(
      (p) => JSON.stringify(c.requests.get(p)) !== JSON.stringify(c.saved.get(p)),
    );
    return [
      ...(c.created ? ["opencollection.yml"] : []),
      ...requests,
      ...[...c.envDirty].map((n) => `environments/${n}.yml`),
      ...(extra ? [...this.host.extraDirty()] : []),
    ];
  }

  /** Saves one collection (`extra`: also the active collection's MCP tests and companion
   *  files). Files are kept under the collection's folder in `MockBackend.files`. */
  saveOf(c: MockCollection, extra: boolean): FileChange[] {
    const changes: FileChange[] = [];
    const at = (path: string) => (c.dir ? `${c.dir}/${path}` : path);
    if (c.created) {
      const text = `# mock rendering\nname: ${c.info.name}\n`;
      this.host.files.set(at("opencollection.yml"), text);
      changes.push({ path: "opencollection.yml", text, deleted: false });
      c.created = false;
    }
    for (const path of this.dirtyOf(c, false).filter((p) => c.requests.has(p))) {
      const spec = c.requests.get(path)!;
      // Simplified rendering; secret values are never part of a request spec.
      const text = `# mock rendering, not OpenCollection\n${JSON.stringify(spec, null, 2)}\n`;
      this.host.files.set(at(path), text);
      c.saved.set(path, structuredClone(spec));
      changes.push({ path, text, deleted: false });
    }
    for (const name of c.envDirty) {
      const e = c.envs.find((x) => x.name === name);
      const path = `environments/${name}.yml`;
      // Secret values are never part of an Environment here (value: null).
      const text = e ? `# mock rendering\n${JSON.stringify(e, null, 2)}\n` : "";
      this.host.files.set(at(path), text);
      changes.push({ path, text, deleted: !e });
    }
    c.envDirty.clear();
    if (extra) changes.push(...this.host.saveExtra());
    return changes;
  }

  private entry(c: MockCollection): CollectionEntry {
    return {
      id: c.info.id,
      name: c.info.name,
      location: c.info.location,
      root: c.root,
      dir: c.dir,
      repo: c.repo,
      dirty: this.dirtyOf(c, c.info.id === this.active).length,
    };
  }

  private set(): CollectionSet {
    return {
      roots: this.roots.map((r) => ({ ...r, collections: [...r.collections], truncated: false })),
      collections: this.roots.flatMap((r) => r.collections.map((id) => this.entry(this.collection(id)))),
      active: this.active,
    };
  }

  async current(): Promise<CollectionSet> {
    return this.set();
  }

  async add(): Promise<CollectionSet | null> {
    if (!this.isOpen) {
      this.openDemo();
      return this.set();
    }
    if (!this.roots.some((r) => r.id === "mock://payments")) {
      const r = (name: string, method: string, url: string): RequestSpec => ({ ...this.host.empty(name), method, url });
      const c = this.make(
        { id: PAYMENTS_ID, name: "Payments API", location: "mock://payments" },
        "mock://payments",
        "",
        new Map([
          ["charges/create-charge.yml", r("Create charge", "POST", "{{baseUrl}}/charges")],
          ["health.yml", r("Payments health", "GET", "{{baseUrl}}/payments/health")],
        ]),
        [env("Local", "http://127.0.0.1:47811")],
      );
      c.repo = null;
      this.data.set(PAYMENTS_ID, c);
      this.roots.push({ id: "mock://payments", name: "payments", location: "mock://payments", collections: [PAYMENTS_ID] });
      this.active = PAYMENTS_ID;
    }
    return this.set();
  }

  async removeRoot(root: string): Promise<CollectionSet> {
    const r = this.roots.find((x) => x.id === root);
    if (!r) return this.set();
    const dirty = r.collections.find((id) => this.dirtyOf(this.collection(id), id === this.active).length > 0);
    if (dirty) {
      throw new BackendError("shell", `"${this.collection(dirty).info.name}" has unsaved changes; save them before removing the folder`);
    }
    this.roots = this.roots.filter((x) => x !== r);
    for (const id of r.collections) this.data.delete(id);
    if (this.active && !this.data.has(this.active)) this.active = this.roots[0]?.collections[0] ?? null;
    return this.set();
  }

  async activate(id: string): Promise<CollectionInfo> {
    const c = this.collection(id);
    this.active = id;
    return c.info;
  }

  async tree(id: string): Promise<TreeNode> {
    const c = this.collection(id);
    return mockTree(c.info.name, c.requests);
  }

  async dirty(id: string): Promise<string[]> {
    return this.dirtyOf(this.collection(id), id === this.active);
  }

  async saveAll(): Promise<CollectionChanges[]> {
    const out: CollectionChanges[] = [];
    for (const r of this.roots) {
      for (const id of r.collections) {
        const c = this.collection(id);
        const changes = this.dirtyOf(c, id === this.active).length > 0 ? this.saveOf(c, id === this.active) : [];
        if (changes.length > 0) out.push({ collection: id, changes });
      }
    }
    return out;
  }

  async search(query: string, limit: number) {
    const rank = (f: string) => (f === "name" ? 0 : f === "url" ? 1 : 2);
    const all = this.roots.flatMap((r) =>
      r.collections.flatMap((id) => mockSearch(this.collection(id).requests, query, limit).map((hit) => ({ ...hit, collection: id }))),
    );
    return all.map((h, i) => ({ h, i })).sort((a, b) => rank(a.h.field) - rank(b.h.field) || a.i - b.i).slice(0, limit).map(({ h }) => h);
  }

  async create(spec: NewCollection): Promise<CollectionInfo | null> {
    const name = spec.name.trim();
    if (!name) throw new BackendError("shell", "a new collection needs a name");
    let rootId = spec.root;
    if (rootId === null) {
      // "Elsewhere…": the mock's folder picker always picks `mock://elsewhere`.
      rootId = "mock://elsewhere";
      if (!this.roots.some((r) => r.id === rootId)) this.roots.push({ id: rootId, name: "elsewhere", location: rootId, collections: [] });
    }
    const root = this.roots.find((r) => r.id === rootId);
    if (!root) throw new BackendError("shell", `no folder ${rootId} in the sidebar`);
    const dir = spec.dir.trim().replace(/^\/+|\/+$/g, "");
    if (dir.split("/").some((p) => p === ".." || p.startsWith(".") || p === "node_modules" || p === "target")) {
      throw new BackendError("shell", `cannot create a collection in "${spec.dir}"`);
    }
    const id = dir ? `${root.id === "mock://demo" ? DEMO_ID : root.id}/${dir}` : root.id;
    if (this.data.has(id) || (root.id === "mock://demo" && dir === "")) {
      throw new BackendError("shell", `${root.location}/${dir} already has files in it; choose another folder`);
    }
    const envs: Environment[] = [];
    const baseUrl = spec.base_url?.trim() || null;
    if (spec.environment?.trim()) {
      envs.push({
        name: spec.environment.trim(),
        client_certificates: [],
        external_secrets: null,
        variables: baseUrl ? [{ name: "baseUrl", value: baseUrl, enabled: true, secret: false }] : [],
      });
    }
    const c = this.make({ id, name, location: `${root.location}${dir ? `/${dir}` : ""}` }, root.id, dir, new Map(), envs);
    c.created = true;
    c.repo = root.id === "mock://demo" ? "mock://demo" : null;
    for (const e of envs) c.envDirty.add(e.name);
    this.data.set(id, c);
    root.collections.push(id);
    this.active = id;
    return c.info;
  }
}
