// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The collections in the sidebar for the web app (wave 14, strand A): `CollectionSetApi` over
// File System Access handles. A folder the user opens or adds is a *root* (its handle is
// remembered in IndexedDB, ./fs/recent.ts); the collections in it are found with the core's one
// rule (`rt_app::collections::{enter, discover}` through rt-wasm `collectionsEnter`,
// `collectionsDiscover`). Each collection is read into its own core `Workspace` only when
// something asks for it (the active one at once, the others when their tree, search or save
// needs them).
//
// Ids: the root's id (random, stable per folder, `fs/recent.ts`) for a root that is a
// collection, else `<root id>/<dir>`. The same collection opened once directly and once through
// its parent folder therefore has two ids (secrets and tokens are not shared; a known limit).
//
// A new collection (`create`) gets its empty folder at once (a handle needs a folder); its files
// are written on the first save, like any edit.

import {
  BackendError,
  type CollectionChanges,
  type CollectionEntry,
  type CollectionInfo,
  type CollectionSearchHit,
  type CollectionSet,
  type CollectionSetApi,
  type FileChange,
  type NewCollection,
  type SearchHit,
  type SecretsApi,
  type TreeNode,
} from "@requesttailor/ui";

import { COLLECTION_FILE, pathParts, readDirectory } from "./fs/directory";
import type { CoreModule, CoreWorkspace } from "./wasm";

/** The core's collection functions (crates/rt-wasm/src/collections_js.rs). */
export interface CollectionsCore {
  collectionsEnter(name: string, depth: number): boolean;
  collectionsDiscover(manifestDirs: string[]): { dirs: string[]; truncated: boolean };
  collectionsCheckDir(dir: string): string;
  collectionsScaffold(spec: NewCollection): FileChange[];
  collectionsMergeHits(perCollection: [string, SearchHit[]][], limit: number): CollectionSearchHit[];
}

export type Core = CoreModule & CollectionsCore;

/** One open collection: its folder, its core workspace and (lazily) its secret store. */
export interface WebSession {
  id: string;
  handle: FileSystemDirectoryHandle;
  ws: CoreWorkspace;
  secrets: Promise<SecretsApi> | null;
  /** The files of a new collection, written on the first save. */
  created?: FileChange[];
}

interface Root {
  id: string;
  name: string;
  handle: FileSystemDirectoryHandle;
  collections: string[];
  truncated: boolean;
}

interface Entry {
  id: string;
  name: string;
  location: string;
  root: string;
  dir: string;
  repo: string | null;
  handle: FileSystemDirectoryHandle;
}

/** What the set needs from the backend. */
export interface WebCollectionsHost {
  loadCore(): Promise<Core>;
  /** Asks for read/write access to the folder if needed. */
  ensureReadWrite(handle: FileSystemDirectoryHandle): Promise<void>;
  /** The stable id of a root folder (remembered with its handle). */
  rememberFolder(handle: FileSystemDirectoryHandle): Promise<string>;
  /** A root was opened or added (recent list). */
  opened(info: CollectionInfo, handle: FileSystemDirectoryHandle): void;
  /** The roots and the active collection changed (remembered per browser profile). */
  remember(roots: string[], active: string | null): void;
  /** A new session (the backend owns its type). */
  session(id: string, handle: FileSystemDirectoryHandle, ws: CoreWorkspace): WebSession;
  /** Shows the folder picker; `null` if cancelled. */
  pick(id: string): Promise<FileSystemDirectoryHandle | null>;
  /** Writes `changes` to the active session and forgets them (the backend's `save`). */
  saveActive(): Promise<FileChange[]>;
}

async function child(dir: FileSystemDirectoryHandle, name: string): Promise<FileSystemDirectoryHandle | null> {
  try {
    return await dir.getDirectoryHandle(name);
  } catch {
    return null;
  }
}

async function has(dir: FileSystemDirectoryHandle, name: string, kind: "file" | "directory" | "any" = "file"): Promise<boolean> {
  for (const k of kind === "any" ? (["file", "directory"] as const) : [kind]) {
    try {
      if (k === "file") await dir.getFileHandle(name);
      else await dir.getDirectoryHandle(name);
      return true;
    } catch {
      // Try the other kind.
    }
  }
  return false;
}

/** The folder `dir` (relative, `/`-separated, `""` = `root`) below `root`. */
export async function folderAt(
  root: FileSystemDirectoryHandle,
  dir: string,
  create = false,
): Promise<FileSystemDirectoryHandle> {
  let h = root;
  if (!dir) return h;
  for (const part of pathParts(dir)) h = await h.getDirectoryHandle(part, { create });
  return h;
}

/** Directories (relative to `root`) that hold an `opencollection.yml`, as a walk with the
 *  core's `enter` finds them (never more than one past the limit). */
export async function manifestDirs(core: CollectionsCore, root: FileSystemDirectoryHandle, limit = 65): Promise<string[]> {
  const found: string[] = [];
  if (await has(root, COLLECTION_FILE)) found.push("");
  const walk = async (dir: FileSystemDirectoryHandle, prefix: string, depth: number): Promise<void> => {
    const subs: FileSystemDirectoryHandle[] = [];
    for await (const entry of dir.values()) {
      if (entry.kind === "directory" && core.collectionsEnter(entry.name, depth)) subs.push(entry as FileSystemDirectoryHandle);
    }
    subs.sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
    for (const sub of subs) {
      if (found.length > limit) return;
      const rel = prefix ? `${prefix}/${sub.name}` : sub.name;
      if (await has(sub, COLLECTION_FILE)) found.push(rel);
      try {
        await walk(sub, rel, depth + 1);
      } catch {
        // A folder that cannot be read is left out.
      }
    }
  };
  await walk(root, "", 1);
  return found;
}

/** The directory (relative to the root) of the Git repository holding `dir`, looking for
 *  `.git` in it and its parents up to the root; `null` outside Git (or above the root). */
async function repoDir(root: FileSystemDirectoryHandle, dir: string): Promise<string | null> {
  const parts = dir ? dir.split("/") : [];
  for (let n = parts.length; n >= 0; n--) {
    const d = parts.slice(0, n).join("/");
    try {
      if (await has(await folderAt(root, d), ".git", "any")) return d;
    } catch {
      // Gone meanwhile.
    }
  }
  return null;
}

const idOf = (root: string, dir: string) => (dir ? `${root}/${dir}` : root);

/** Whether two handles are the same folder. */
async function same(a: FileSystemDirectoryHandle, b: FileSystemDirectoryHandle): Promise<boolean> {
  if (a === b) return true;
  return typeof a.isSameEntry === "function" ? a.isSameEntry(b).catch(() => false) : false;
}

export class WebCollectionSet implements CollectionSetApi {
  private roots: Root[] = [];
  private entries: Entry[] = [];
  private readonly sessions = new Map<string, WebSession>();
  private active: string | null = null;

  constructor(private readonly host: WebCollectionsHost) {}

  // ---- for the backend ----

  /** The active collection's session; throws `BackendError` if none. */
  session(): WebSession {
    const s = this.active ? this.sessions.get(this.active) : undefined;
    if (!s) throw new BackendError("shell", "No collection is open.");
    return s;
  }

  get isOpen(): boolean {
    return this.active !== null && this.sessions.has(this.active);
  }

  info(id: string): CollectionInfo {
    const e = this.entry(id);
    const s = this.sessions.get(id);
    return { id, name: s?.ws.name().trim() || e.name, location: e.location };
  }

  /** Whether `handle` is a root or a collection folder of the set. */
  async contains(handle: FileSystemDirectoryHandle): Promise<boolean> {
    for (const h of [...this.roots.map((r) => r.handle), ...this.entries.map((e) => e.handle)]) {
      if (await same(h, handle)) return true;
    }
    return false;
  }

  /** Opens what `handle` holds as the only root (the folder the user picked), its first
   *  collection active. `keep`: stop before replacing anything if it says so (a collection
   *  was opened meanwhile). */
  async openRoot(handle: FileSystemDirectoryHandle, keep?: () => boolean): Promise<CollectionInfo | null> {
    await this.host.ensureReadWrite(handle);
    const core = await this.host.loadCore();
    const found = await this.find(core, handle, "");
    const first = found.entries[0];
    const session = await this.read(core, first.id, first.handle);
    if (keep?.()) {
      session.ws.free();
      return null;
    }
    // The previous workspaces are not freed here: a send that started before may still hold
    // one (freeing made it fail with "null pointer passed to rust"); wasm-bindgen's
    // FinalizationRegistry frees them once nothing refers to them any more.
    this.roots = [found.root];
    this.entries = found.entries;
    this.sessions.clear();
    this.sessions.set(first.id, session);
    this.active = first.id;
    this.host.opened(this.rootInfo(found.root), handle);
    this.remember();
    return this.info(first.id);
  }

  /** Reads the collections of the active collection's repository again (after Git changed
   *  files), and finds the roots' collections again. */
  async reload(): Promise<CollectionInfo> {
    const active = this.active;
    if (!active) throw new BackendError("shell", "No collection is open.");
    const core = await this.host.loadCore();
    const repo = this.entry(active).repo;
    const mates = new Set(this.entries.filter((e) => (repo ? e.repo === repo : e.id === active)).map((e) => e.id));
    const entries: Entry[] = [];
    const roots: Root[] = [];
    for (const root of this.roots) {
      try {
        const found = await this.find(core, root.handle, root.id);
        roots.push(found.root);
        entries.push(...found.entries.filter((e) => !entries.some((x) => x.id === e.id)));
      } catch {
        roots.push(root);
        entries.push(...this.entries.filter((e) => e.root === root.id));
      }
    }
    // New collections not saved yet have no manifest on disk: they stay.
    for (const e of this.entries) {
      if (this.sessions.get(e.id)?.created && !entries.some((x) => x.id === e.id)) {
        entries.push(e);
        roots.find((r) => r.id === e.root)?.collections.push(e.id);
      }
    }
    for (const [id, s] of [...this.sessions]) {
      if (!entries.some((e) => e.id === id) || (mates.has(id) && !s.created)) this.sessions.delete(id);
    }
    this.roots = roots;
    this.entries = entries;
    this.active = entries.some((e) => e.id === active) ? active : (entries[0]?.id ?? null);
    if (!this.active) throw new BackendError("shell", "No collection is left in the sidebar.");
    await this.loaded(this.active);
    return this.info(this.active);
  }

  /** Restores the remembered roots (`ids`, with their handles from `folder`) if `id` is one
   *  of them; `null` otherwise or if none can be opened without asking. */
  async restore(
    id: string,
    saved: { roots: string[]; active: string | null },
    folder: (id: string) => Promise<FileSystemDirectoryHandle | null>,
    keep?: () => boolean,
  ): Promise<CollectionInfo | null> {
    if (!saved.roots.includes(id)) return null;
    let first = true;
    for (const rootId of saved.roots) {
      const handle = await folder(rootId);
      if (!handle) continue;
      try {
        if (first) {
          if (!(await this.openRoot(handle, keep))) return null;
          first = false;
        } else {
          await this.addRoot(handle, false);
        }
      } catch (e) {
        console.warn("RequestTailor: a remembered folder could not be reopened:", e);
      }
    }
    if (first) return null;
    const active = saved.active && this.entries.some((e) => e.id === saved.active) ? saved.active : this.entries[0]?.id;
    return active ? this.activate(active) : null;
  }

  // ---- CollectionSetApi ----

  async current(): Promise<CollectionSet> {
    return this.set();
  }

  async add(): Promise<CollectionSet | null> {
    const handle = await this.host.pick("requesttailor-add-collection");
    if (!handle) return null;
    if (this.roots.length === 0) {
      await this.openRoot(handle);
      return this.set();
    }
    await this.addRoot(handle, true);
    return this.set();
  }

  /** Adds `handle` as another root (`activate`: its first new collection becomes active). */
  async addRoot(handle: FileSystemDirectoryHandle, activate: boolean, allowEmpty = false): Promise<string> {
    for (const r of this.roots) if (await same(r.handle, handle)) return r.id;
    await this.host.ensureReadWrite(handle);
    const core = await this.host.loadCore();
    const found = await this.find(core, handle, "", allowEmpty, true);
    const id = found.root.id;
    const fresh = found.entries.filter((e) => !this.entries.some((x) => x.id === e.id));
    if (fresh.length === 0 && !allowEmpty) return id;
    if (fresh[0]) await this.read(core, fresh[0].id, fresh[0].handle).then((s) => this.sessions.set(s.id, s));
    found.root.collections = fresh.map((e) => e.id);
    this.roots.push(found.root);
    this.entries.push(...fresh);
    if (activate && fresh[0]) this.active = fresh[0].id;
    this.host.opened(this.rootInfo(found.root), handle);
    this.remember();
    return id;
  }

  async removeRoot(root: string): Promise<CollectionSet> {
    const r = this.roots.find((x) => x.id === root);
    if (!r) return this.set();
    for (const id of r.collections) {
      if (this.dirtyNow(id).length > 0) {
        throw new BackendError("shell", `"${this.info(id).name}" has unsaved changes; save them before removing the folder`);
      }
    }
    this.roots = this.roots.filter((x) => x !== r);
    this.entries = this.entries.filter((e) => e.root !== root);
    for (const id of r.collections) this.sessions.delete(id);
    if (this.active && !this.entries.some((e) => e.id === this.active)) {
      this.active = this.entries[0]?.id ?? null;
      if (this.active) await this.loaded(this.active);
    }
    this.remember();
    return this.set();
  }

  async activate(id: string): Promise<CollectionInfo> {
    await this.loaded(id);
    this.active = id;
    this.remember();
    return this.info(id);
  }

  async tree(id: string): Promise<TreeNode> {
    return (await this.loaded(id)).ws.tree();
  }

  async dirty(id: string): Promise<string[]> {
    await this.loaded(id);
    return this.dirtyNow(id);
  }

  async saveAll(): Promise<CollectionChanges[]> {
    const out: CollectionChanges[] = [];
    const active = this.active;
    try {
      for (const e of this.entries) {
        if (!this.sessions.has(e.id) || this.dirtyNow(e.id).length === 0) continue;
        // Saving goes through the backend's `save` on the session, which writes and commits.
        this.active = e.id;
        const changes = await this.host.saveActive();
        if (changes.length > 0) out.push({ collection: e.id, changes });
      }
    } finally {
      this.active = active;
    }
    return out;
  }

  async search(query: string, limit: number): Promise<CollectionSearchHit[]> {
    const core = await this.host.loadCore();
    const per: [string, SearchHit[]][] = [];
    for (const e of this.entries) {
      try {
        per.push([e.id, (await this.loaded(e.id)).ws.search(query, limit)]);
      } catch {
        // A collection that cannot be read has no hits; its node shows the error.
      }
    }
    return core.collectionsMergeHits(per, limit);
  }

  async create(spec: NewCollection): Promise<CollectionInfo | null> {
    const core = await this.host.loadCore();
    const dir = core.collectionsCheckDir(spec.dir);
    const files = core.collectionsScaffold(spec);
    let rootId = spec.root;
    if (rootId === null) {
      const handle = await this.host.pick("requesttailor-new-collection");
      if (!handle) return null;
      rootId = await this.addRoot(handle, false, true);
    }
    const root = this.roots.find((r) => r.id === rootId);
    if (!root) throw new BackendError("shell", `No folder ${rootId} in the sidebar.`);
    const id = idOf(root.id, dir);
    if (this.entries.some((e) => e.id === id)) throw new BackendError("shell", `${root.name}/${dir} is a collection already.`);
    let existing: FileSystemDirectoryHandle | null = root.handle;
    for (const part of dir ? dir.split("/") : []) existing = existing ? await child(existing, part) : null;
    if (existing) {
      for await (const _ of existing.values()) {
        throw new BackendError("shell", `${root.name}${dir ? `/${dir}` : ""} already has files in it; choose another folder.`);
      }
    }
    await this.host.ensureReadWrite(root.handle);
    const handle = await folderAt(root.handle, dir, true);
    const ws = core.Workspace.open(new Map(files.map((f) => [f.path, new TextEncoder().encode(f.text)])));
    const session = this.host.session(id, handle, ws);
    session.created = files;
    const repo = await repoDir(root.handle, dir);
    this.sessions.set(id, session);
    this.entries.push({
      id,
      name: spec.name.trim(),
      location: `${root.name}${dir ? `/${dir}` : ""}`,
      root: root.id,
      dir,
      repo: repo === null ? null : idOf(root.id, repo),
      handle,
    });
    root.collections.push(id);
    // Sidebar order: roots in order, each root's collections in order.
    const order = this.roots.flatMap((r) => r.collections);
    this.entries.sort((a, b) => order.indexOf(a.id) - order.indexOf(b.id));
    this.active = id;
    this.remember();
    return this.info(id);
  }

  // ---- internals ----

  private entry(id: string): Entry {
    const e = this.entries.find((x) => x.id === id);
    if (!e) throw new BackendError("shell", `No collection ${id} in the sidebar.`);
    return e;
  }

  /** Unsaved paths of a session that was read (none otherwise). */
  dirtyNow(id: string): string[] {
    const s = this.sessions.get(id);
    if (!s) return [];
    const dirty = s.ws.dirty();
    for (const f of s.created ?? []) if (!dirty.includes(f.path)) dirty.push(f.path);
    return dirty.sort();
  }

  private async read(core: Core, id: string, handle: FileSystemDirectoryHandle): Promise<WebSession> {
    const ws = core.Workspace.open(await readDirectory(handle));
    return this.host.session(id, handle, ws);
  }

  /** The session of `id`, read on first use. */
  private async loaded(id: string): Promise<WebSession> {
    const known = this.sessions.get(id);
    if (known) return known;
    const e = this.entry(id);
    const s = await this.read(await this.host.loadCore(), id, e.handle);
    // Read twice meanwhile: keep the first.
    const raced = this.sessions.get(id);
    if (raced) {
      s.ws.free();
      return raced;
    }
    this.sessions.set(id, s);
    return s;
  }

  /** The collections in `handle` (`rootId` empty: remember the folder for a new id). A folder
   *  without any collection is one, unless `allowEmpty` (a root for a new collection). */
  private async find(
    core: Core,
    handle: FileSystemDirectoryHandle,
    rootId: string,
    allowEmpty = false,
    refuseEmpty = false,
  ): Promise<{ root: Root; entries: Entry[] }> {
    const dirs = await manifestDirs(core, handle);
    if (dirs.length === 0 && refuseEmpty && !allowEmpty) {
      throw new BackendError("shell", `There is no collection in "${handle.name}"; create one with New… → Collection.`);
    }
    const id = rootId || (await this.host.rememberFolder(handle));
    const discovery = core.collectionsDiscover(dirs);
    const list = dirs.length === 0 && allowEmpty ? [] : discovery.dirs;
    const entries: Entry[] = [];
    for (const dir of list) {
      const h = await folderAt(handle, dir);
      const repo = await repoDir(handle, dir);
      entries.push({
        id: idOf(id, dir),
        name: (await manifestName(core, h)) ?? (dir ? dir.split("/").pop()! : handle.name),
        location: dir ? `${handle.name}/${dir}` : handle.name,
        root: id,
        dir,
        repo: repo === null ? null : idOf(id, repo),
        handle: h,
      });
    }
    return {
      root: { id, name: handle.name, handle, collections: entries.map((e) => e.id), truncated: discovery.truncated },
      entries,
    };
  }

  private rootInfo(root: Root): CollectionInfo {
    const only = this.entries.length > 0 && root.collections.length === 1 ? this.entries.find((e) => e.id === root.collections[0]) : null;
    const name = only && only.dir === "" ? this.info(only.id).name : root.name;
    return { id: root.id, name, location: root.name };
  }

  private set(): CollectionSet {
    const entry = (e: Entry): CollectionEntry => ({
      id: e.id,
      name: this.info(e.id).name,
      location: e.location,
      root: e.root,
      dir: e.dir,
      repo: e.repo,
      dirty: this.dirtyNow(e.id).length,
    });
    return {
      roots: this.roots.map((r) => ({
        id: r.id,
        name: r.name,
        location: r.name,
        collections: [...r.collections],
        truncated: r.truncated,
      })),
      collections: this.entries.map(entry),
      active: this.active,
    };
  }

  private remember(): void {
    this.host.remember(
      this.roots.map((r) => r.id),
      this.active,
    );
  }
}

/** `info.name` of a collection's `opencollection.yml` (only that file is read). */
async function manifestName(core: Core, dir: FileSystemDirectoryHandle): Promise<string | null> {
  try {
    const file = await (await dir.getFileHandle(COLLECTION_FILE)).getFile();
    const ws = core.Workspace.open(new Map([[COLLECTION_FILE, new Uint8Array(await file.arrayBuffer())]]));
    try {
      return ws.name().trim() || null;
    } finally {
      ws.free();
    }
  } catch {
    return null;
  }
}
