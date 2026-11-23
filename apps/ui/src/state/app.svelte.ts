// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Shared app state (Svelte 5 runes). Every strand reads it; changes go through these methods
// so behaviour stays the same in every component. Components get it with `getApp()`.
// Adding fields or methods is an interface change (working rule 1).

import { getContext, setContext } from "svelte";

import type { Backend, CollectionInfo } from "../commands/backend";
import type {
  CollectionEntry,
  Environment,
  NewCollection,
  RequestRef,
  RequestSpec,
  StoredResponse,
  TreeNode,
} from "../commands/types";
import type { ExtensionDialog } from "../extensions";

const KEY = Symbol("requesttailor-app");

/** Wave 14: a view of the main area that extensions and the "New…" dialog can open. */
export type MainView =
  /** A collection tab: `<extension id>/<tab id>` for an extension's tab, or the palette key of
   *  a plugin's collection tab. Nothing happens if no such tab is offered right now. */
  | { kind: "collection-tab"; key: string }
  /** The MCP client. */
  | { kind: "mcp" }
  /** The runner for `folder` of the active collection (`null`: the whole collection). */
  | { kind: "runner"; folder: string | null };

/** Wave 14: what the "New…" dialog opens with. */
export interface NewDialogRequest {
  /** A built-in kind (`new/kinds.ts`) or an extension's tile `<extension id>/<item id>` to
   *  start on its form; absent: the tiles. */
  kind?: string;
  /** The collection and folder the forms start with. */
  collection: string | null;
  folder: string;
  /** Counts requests, so asking again while open restarts it. */
  seq: number;
}

/** Wave 14: the dialog of an extension the shell shows (`showDialog`). */
export interface OpenDialog {
  dialog: ExtensionDialog;
  /** Counts dialogs, a key for the host. */
  seq: number;
  resolve: (result: unknown) => void;
}

export class AppState {
  readonly backend: Backend;

  collection = $state<CollectionInfo | null>(null);
  tree = $state<TreeNode | null>(null);
  /** Open tabs (request paths), in tab order. */
  tabs = $state<string[]>([]);
  /** The request shown in the editor. */
  active = $state<string | null>(null);
  /** Current editor state per open request; sent and saved from here. */
  drafts = $state<Record<string, RequestSpec>>({});
  /** Paths with unsaved changes (backend's view after the last edit). */
  dirty = $state<string[]>([]);
  environments = $state<Environment[]>([]);
  environment = $state<string | null>(null);
  /** Last response per request path. */
  results = $state<Record<string, StoredResponse>>({});
  sending = $state<Record<string, boolean>>({});
  /** Last error to show in the status area, or `null`. */
  error = $state<string | null>(null);
  /** Counts finished sends; views that show sent data (history) re-read when it changes. */
  sendCount = $state(0);
  /** Counts collection (re)loads: open, `adopt`, `reload`; views that cache collection data
   *  (Git status, search results) re-read when it changes. */
  loadCount = $state(0);

  // ---- wave 14 (interface): several collections ----
  // Strand A turns the fields above into the state of the *active* collection (the collection
  // of the active tab, or the node chosen last): `collection`, `tree`, `tabs` (the active
  // collection's open tabs), `active`, `drafts`, `dirty`, `environments`, `environment`,
  // `results` and `sending` keep their meaning for it, so extensions keep working unchanged.

  /** The collections in the sidebar, in order. Until strand A: the open collection. */
  collections = $state<CollectionEntry[]>([]);
  /** Trees of the collections in the sidebar, by id. */
  trees = $state<Record<string, TreeNode>>({});
  /** The tab bar across collections, in tab order. Until strand A: the open collection's. */
  openTabs = $derived<RequestRef[]>(
    this.collection ? this.tabs.map((path) => ({ collection: this.collection!.id, path })) : [],
  );
  /** A view of the main area asked for with `show` / `openCollectionTab`; the shell takes it. */
  view = $state<{ view: MainView; seq: number } | null>(null);
  /** The "New…" dialog while open (`openNew`). */
  newDialog = $state<NewDialogRequest | null>(null);
  /** An extension's dialog while open (`showDialog`). */
  dialog = $state<OpenDialog | null>(null);
  private seq = 0;

  /** Resolves when every `edit` so far has reached the backend; `send`/`save` wait for it so
   *  shells with concurrent commands (Tauri) never send or save a stale request. */
  private lastEdit: Promise<unknown> = Promise.resolve();

  /** Wave 10: listeners of `onEdit`. */
  private readonly editListeners = new Set<(path: string, spec: RequestSpec) => void>();

  constructor(backend: Backend) {
    this.backend = backend;
  }

  private async guard<T>(f: () => Promise<T>): Promise<T | undefined> {
    try {
      this.error = null;
      return await f();
    } catch (e) {
      this.error = e instanceof Error ? e.message : String(e);
      return undefined;
    }
  }

  /** Wave 14: id of the active collection. */
  get activeCollection(): string | null {
    return this.collection?.id ?? null;
  }

  /** Wave 14: makes collection `id` active (its tabs, environments, Git, ...). */
  async activate(id: string): Promise<void> {
    if (id === this.collection?.id) return;
    await this.lastEdit;
    await this.guard(async () => {
      const info = await this.backend.collections.activate(id);
      await this.load(info);
    });
  }

  /** Wave 14: opens a request of any collection in the sidebar in a tab. */
  async selectRef(ref: RequestRef): Promise<void> {
    await this.activate(ref.collection);
    if (this.collection?.id === ref.collection) await this.select(ref.path);
  }

  /** Wave 14: "Add collection…": a folder picker, its collections join the sidebar. */
  async addCollection(): Promise<void> {
    await this.guard(async () => {
      const set = await this.backend.collections.add();
      if (set?.active && set.active !== this.collection?.id) await this.activate(set.active);
    });
  }

  /** Wave 14: creates a collection (unsaved) and makes it active; `null` if cancelled. */
  async createCollection(spec: NewCollection): Promise<CollectionInfo | null> {
    return (
      (await this.guard(async () => {
        const info = await this.backend.collections.create(spec);
        if (info) await this.load(info);
        return info;
      })) ?? null
    );
  }

  /** Wave 14: asks the shell to show a view of the main area. */
  show(view: MainView): void {
    this.view = { view, seq: ++this.seq };
  }

  /** Wave 14: shows a collection tab (`<extension id>/<tab id>`) of the active collection. */
  openCollectionTab(key: string): void {
    this.show({ kind: "collection-tab", key });
  }

  /** Wave 14: opens the "New…" dialog (default: the active collection's root). */
  openNew(options: { kind?: string; collection?: string | null; folder?: string } = {}): void {
    this.newDialog = {
      kind: options.kind,
      collection: options.collection === undefined ? this.activeCollection : options.collection,
      folder: options.folder ?? "",
      seq: ++this.seq,
    };
  }

  closeNew(): void {
    this.newDialog = null;
  }

  /** Wave 14: shows an extension's dialog; resolves with what it passes to `close` (closing
   *  with Escape or the close button: `undefined`). A second dialog replaces the first, which
   *  resolves `undefined`. */
  showDialog(dialog: ExtensionDialog): Promise<unknown> {
    this.dialog?.resolve(undefined);
    return new Promise((resolve) => {
      this.dialog = { dialog, seq: ++this.seq, resolve };
    });
  }

  closeDialog(result?: unknown): void {
    const open = this.dialog;
    this.dialog = null;
    open?.resolve(result);
  }

  /** Until strand A: the sidebar's collections are the open one. */
  private syncSingle(): void {
    const info = this.collection;
    if (!info) {
      this.collections = [];
      this.trees = {};
      return;
    }
    this.collections = [
      { id: info.id, name: info.name, location: info.location, root: info.id, dir: "", repo: null, dirty: this.dirty.length },
    ];
    this.trees = this.tree ? { [info.id]: this.tree } : {};
  }

  /** Shows the folder picker and opens the chosen collection. */
  async open(): Promise<void> {
    await this.guard(async () => {
      const info = await this.backend.openCollection();
      if (info) await this.load(info);
    });
  }

  /** Shows a collection the backend has just opened by other means (recent list, import,
   *  clone). */
  async adopt(info: CollectionInfo): Promise<void> {
    await this.guard(() => this.load(info));
  }

  private async load(info: CollectionInfo): Promise<void> {
    this.collection = info;
    this.tabs = [];
    this.active = null;
    this.drafts = {};
    this.results = {};
    this.tree = await this.backend.tree();
    this.environments = await this.backend.environments();
    this.environment = this.environments[0]?.name ?? null;
    this.dirty = await this.backend.dirty();
    this.syncSingle();
    this.loadCount++;
  }

  /** Re-reads the open collection after its files changed underneath (Git checkout, pull,
   *  conflict resolution). Keeps the tabs whose requests still exist, with their state from
   *  the files; the active environment stays if it still exists. */
  async reload(): Promise<void> {
    if (!this.collection) return;
    await this.lastEdit;
    await this.guard(async () => {
      this.tree = await this.backend.tree();
      const drafts: Record<string, RequestSpec> = {};
      for (const path of this.tabs) {
        try {
          drafts[path] = await this.backend.getRequest(path);
        } catch {
          // The request is gone in this version of the files.
        }
      }
      this.drafts = drafts;
      this.tabs = this.tabs.filter((p) => p in drafts);
      if (this.active && !(this.active in drafts)) this.active = this.tabs[0] ?? null;
      this.environments = await this.backend.environments();
      if (!this.environments.some((e) => e.name === this.environment)) {
        this.environment = this.environments[0]?.name ?? null;
      }
      this.dirty = await this.backend.dirty();
      this.syncSingle();
      this.loadCount++;
    });
  }

  /** Opens a request in a tab (or focuses its tab). */
  async select(path: string): Promise<void> {
    await this.guard(async () => {
      if (!this.drafts[path]) this.drafts[path] = await this.backend.getRequest(path);
      if (!this.tabs.includes(path)) this.tabs = [...this.tabs, path];
      this.active = path;
    });
  }

  close(path: string): void {
    const i = this.tabs.indexOf(path);
    if (i < 0) return;
    this.tabs = this.tabs.filter((p) => p !== path);
    if (this.active === path) this.active = this.tabs[Math.min(i, this.tabs.length - 1)] ?? null;
  }

  /** Records an edit from the editor and tells the backend (in memory, unsaved). */
  async edit(path: string, spec: RequestSpec): Promise<void> {
    this.drafts[path] = spec;
    const snapshot = $state.snapshot(spec) as RequestSpec;
    for (const listener of this.editListeners) {
      try {
        listener(path, snapshot);
      } catch (e) {
        console.warn("RequestTailor: an edit listener failed:", e);
      }
    }
    await this.update(path, snapshot);
  }

  /** Wave 10: calls `listener` after every edit the user makes in the editor (`edit`), with
   *  the request as edited; not for changes applied with `applyExternal`. Returns the
   *  unsubscribe. For extensions that share edits (live collaboration). */
  onEdit(listener: (path: string, spec: RequestSpec) => void): () => void {
    this.editListeners.add(listener);
    return () => this.editListeners.delete(listener);
  }

  /** Wave 10: applies a request that changed elsewhere (another person, another device) as
   *  if it had been edited here: the backend gets it (in memory, unsaved, like `edit`) and an
   *  open tab shows it at once. `onEdit` listeners are not called. Requests that are not
   *  open only reach the backend. */
  async applyExternal(path: string, spec: RequestSpec): Promise<void> {
    if (path in this.drafts) this.drafts[path] = spec;
    await this.update(path, $state.snapshot(spec) as RequestSpec);
  }

  private async update(path: string, snapshot: RequestSpec): Promise<void> {
    const run = this.lastEdit.then(() =>
      this.guard(async () => {
        await this.backend.updateRequest(path, snapshot);
        this.dirty = await this.backend.dirty();
        if (this.tree) this.tree = await this.backend.tree();
        this.syncSingle();
      }),
    );
    this.lastEdit = run;
    await run;
  }

  /** Resolves when every `edit` so far has reached the backend (wave 8: sessions open the
   *  request as edited, like `send`). */
  async settled(): Promise<void> {
    await this.lastEdit;
  }

  async create(folder: string, name: string): Promise<void> {
    await this.guard(async () => {
      const path = await this.backend.createRequest(folder, name);
      this.tree = await this.backend.tree();
      this.dirty = await this.backend.dirty();
      this.syncSingle();
      await this.select(path);
    });
  }

  async save(): Promise<void> {
    await this.lastEdit;
    await this.guard(async () => {
      await this.backend.save();
      this.dirty = await this.backend.dirty();
      this.syncSingle();
    });
  }

  async send(path: string | null = this.active): Promise<void> {
    if (!path) return;
    this.sending[path] = true;
    await this.lastEdit;
    await this.guard(async () => {
      const previous = this.results[path];
      const result = await this.backend.send(path, { environment: this.environment, vars: [] });
      this.results[path] = result;
      if (previous?.response_id != null) await this.backend.releaseResponse(previous.response_id);
    });
    this.sending[path] = false;
    this.sendCount++;
  }

  setEnvironment(name: string | null): void {
    this.environment = name;
  }

  /** Re-reads environments after edits; keeps the active one if it still exists. */
  async reloadEnvironments(): Promise<void> {
    await this.guard(async () => {
      this.environments = await this.backend.environments();
      if (!this.environments.some((e) => e.name === this.environment)) {
        this.environment = this.environments[0]?.name ?? null;
      }
      this.dirty = await this.backend.dirty();
    });
  }
}

export function provideApp(state: AppState): AppState {
  return setContext(KEY, state);
}

export function getApp(): AppState {
  const state = getContext<AppState | undefined>(KEY);
  if (!state) throw new Error("getApp() called outside <AppShell>");
  return state;
}
