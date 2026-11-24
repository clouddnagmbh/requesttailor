// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Shared app state (Svelte 5 runes). Every strand reads it; changes go through these methods
// so behaviour stays the same in every component. Components get it with `getApp()`.
// Adding fields or methods is an interface change (working rule 1).

import { getContext, setContext } from "svelte";

import { BackendError, type Backend, type CollectionInfo } from "../commands/backend";
import type {
  CollectionEntry,
  CollectionRoot,
  CollectionSet,
  Environment,
  NewCollection,
  RepositoryEntry,
  RequestRef,
  RequestSpec,
  StoredResponse,
  TreeNode,
  VarUse,
} from "../commands/types";
import type { ExtensionDialog } from "../extensions";
import type { GitView } from "../git/views.svelte";
import { load as loadSetting, store as storeSetting } from "../shell/storage";

const KEY = Symbol("requesttailor-app");

/** Wave 14: a view of the main area that extensions and the "New…" dialog can open. */
export type MainView =
  /** A collection tab: `<extension id>/<tab id>` for an extension's tab, or the palette key of
   *  a plugin's collection tab. Nothing happens if no such tab is offered right now. */
  | { kind: "collection-tab"; key: string }
  /** The MCP client; with `test`, that saved test (`*.mcp.yaml` path) opened in it. */
  | { kind: "mcp"; test?: string }
  /** The environment manager, on the selected environment. */
  | { kind: "environments" }
  /** The runner for `folder` of the active collection (`null`: the whole collection). */
  | { kind: "runner"; folder: string | null }
  /** Wave 15: a Git view of the active repository (`git/views.svelte.ts`), e.g. an extension's
   *  `{ kind: "extension", id: "<extension>/<view>" }`. */
  | { kind: "git"; view: GitView };

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

/** Wave 14: what `AppState` keeps per collection of the set; the fields of the same name on
 *  `AppState` are the active collection's. */
interface CollectionView {
  active: string | null;
  drafts: Record<string, RequestSpec>;
  dirty: string[];
  environments: Environment[];
  environment: string | null;
  results: Record<string, StoredResponse>;
  sending: Record<string, boolean>;
}

const NO_DRAFTS: Record<string, RequestSpec> = Object.freeze({}) as Record<string, RequestSpec>;
const NO_RESULTS: Record<string, StoredResponse> = Object.freeze({}) as Record<string, StoredResponse>;
const NO_SENDING: Record<string, boolean> = Object.freeze({}) as Record<string, boolean>;

const isName = (v: unknown): v is string => typeof v === "string";

/** What `AppState.onSent` reports (the shell turns it into `AppExtension.onExchange`). */
export interface SentExchange {
  collection: string;
  path: string;
  response: StoredResponse;
  request: RequestSpec;
  variables: VarUse[];
}

export class AppState {
  readonly backend: Backend;

  /** The active collection (wave 14: of the collection set in the sidebar). */
  collection = $state<CollectionInfo | null>(null);

  // ---- wave 14: per collection; the accessors below are the active collection's ----
  /** State of every collection of the set that was shown, by id. */
  private views = $state<Record<string, CollectionView>>({});

  private get current(): CollectionView | undefined {
    const id = this.collection?.id;
    return id ? this.views[id] : undefined;
  }

  /** The active collection's tree. */
  get tree(): TreeNode | null {
    const id = this.collection?.id;
    return (id && this.trees[id]) || null;
  }
  set tree(tree: TreeNode | null) {
    const id = this.collection?.id;
    if (!id) return;
    if (tree) this.trees[id] = tree;
    else delete this.trees[id];
  }
  /** Open tabs of the active collection (request paths), in tab order; `openTabs` has all. */
  get tabs(): string[] {
    const id = this.collection?.id;
    return id ? this.openTabs.filter((t) => t.collection === id).map((t) => t.path) : [];
  }
  /** The request shown in the editor. */
  get active(): string | null {
    return this.current?.active ?? null;
  }
  set active(path: string | null) {
    const view = this.current;
    if (view) view.active = path;
  }
  /** Current editor state per open request of the active collection; sent and saved from here. */
  get drafts(): Record<string, RequestSpec> {
    return this.current?.drafts ?? NO_DRAFTS;
  }
  set drafts(drafts: Record<string, RequestSpec>) {
    const view = this.current;
    if (view) view.drafts = drafts;
  }
  /** Paths with unsaved changes in the active collection (backend's view after the last edit). */
  get dirty(): string[] {
    return this.current?.dirty ?? [];
  }
  set dirty(dirty: string[]) {
    const id = this.collection?.id;
    if (id) this.setDirty(id, dirty);
  }
  get environments(): Environment[] {
    return this.current?.environments ?? [];
  }
  set environments(environments: Environment[]) {
    const view = this.current;
    if (view) view.environments = environments;
  }
  get environment(): string | null {
    return this.current?.environment ?? null;
  }
  set environment(name: string | null) {
    const view = this.current;
    if (view) view.environment = name;
  }
  /** Last response per request path of the active collection. */
  get results(): Record<string, StoredResponse> {
    return this.current?.results ?? NO_RESULTS;
  }
  set results(results: Record<string, StoredResponse>) {
    const view = this.current;
    if (view) view.results = results;
  }
  get sending(): Record<string, boolean> {
    return this.current?.sending ?? NO_SENDING;
  }
  set sending(sending: Record<string, boolean>) {
    const view = this.current;
    if (view) view.sending = sending;
  }
  /** Last error to show in the status area, or `null`. */
  error = $state<string | null>(null);
  /** Counts finished sends; views that show sent data (history) re-read when it changes. */
  sendCount = $state(0);
  /** Counts collection (re)loads: open, `adopt`, `reload`; views that cache collection data
   *  (Git status, search results) re-read when it changes. */
  loadCount = $state(0);

  // ---- wave 14: several collections ----

  /** The collections in the sidebar, in order (`CollectionSetApi.current`). */
  collections = $state<CollectionEntry[]>([]);
  /** The folders they were found in (opened or added), in order. */
  roots = $state<CollectionRoot[]>([]);
  /** Wave 15: the repositories of the set (`CollectionSet.repositories`), in sidebar order. */
  repositories = $state<RepositoryEntry[]>([]);
  /** Wave 15: the repository every Git view acts on (`CollectionSet.active_repository`). */
  activeRepository = $state<string | null>(null);
  /** Trees of the collections in the sidebar that were read, by id (the active one always;
   *  the others once `ensureTree` asked, e.g. when their node opens). */
  trees = $state<Record<string, TreeNode>>({});
  /** The tab bar across collections, in tab order. */
  openTabs = $state<RequestRef[]>([]);
  /** A view of the main area asked for with `show` / `openCollectionTab`; the shell takes it. */
  view = $state<{ view: MainView; seq: number } | null>(null);
  /** The "New…" dialog while open (`openNew`). */
  newDialog = $state<NewDialogRequest | null>(null);
  /** An extension's dialog while open (`showDialog`). */
  dialog = $state<OpenDialog | null>(null);
  /** A request tab asked for with `openRequestTab`; the request editor takes it. */
  requestTab = $state<{ key: string; seq: number } | null>(null);
  private seq = 0;
  /** Asked before every `send` once the request's edits have reached the backend (the
   *  OpenAPI run wizard, `openapi/gate.ts`); `false` cancels the send. Errors let it through. */
  sendGate: ((path: string) => Promise<boolean>) | null = null;
  /** Asked before `sendGate` (wave 17: "Send POST to prod?", `environments/production.svelte.ts`);
   *  `false` cancels the send. Errors let it through. */
  sendConfirm: ((path: string) => Promise<boolean>) | null = null;

  /** Resolves when every `edit` so far has reached the backend; `send`/`save` wait for it so
   *  shells with concurrent commands (Tauri) never send or save a stale request. Wave 14:
   *  switching the backend's active collection runs in the same queue, so an edit always
   *  reaches its own collection. */
  private lastEdit: Promise<unknown> = Promise.resolve();
  /** The collection the backend has active (it may lag behind `collection` while the queue
   *  runs). */
  private backendActive: string | null = null;

  /** Wave 17: listeners of `onSent`. */
  private readonly sentListeners = new Set<(sent: SentExchange) => void>();
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

  /** Runs `f` after everything queued so far (edits, switches); returns its result. */
  private enqueue<T>(f: () => Promise<T>): Promise<T> {
    const run = this.lastEdit.then(f);
    this.lastEdit = run.catch(() => undefined);
    return run;
  }

  /** Makes `id` the backend's active collection (in the queue). */
  private async backendTo(id: string): Promise<CollectionInfo> {
    const info = await this.backend.collections.activate(id);
    this.backendActive = id;
    return info;
  }

  /** Wave 14: makes collection `id` active (its tabs, environments, Git, ...). */
  async activate(id: string): Promise<void> {
    // Wave 15: a collection chosen in the sidebar brings Git back to its repository, also when
    // another repository was chosen meanwhile.
    const repo = this.collections.find((c) => c.id === id)?.repo ?? null;
    const followed = this.activeRepository === null || this.activeRepository === repo;
    if (id === this.collection?.id && id === this.backendActive && followed) return;
    await this.guard(() =>
      this.enqueue(async () => {
        const info = await this.backendTo(id);
        await this.show_(info);
        await this.refreshSet();
      }),
    );
  }

  /** Shows `info` (the backend's active collection) as the active one: its state if it was
   *  shown before, else read now. Runs in the queue. */
  private async show_(info: CollectionInfo): Promise<void> {
    const id = info.id;
    const known = this.views[id];
    if (!known) {
      const [tree, environments, dirty] = await Promise.all([
        this.backend.tree(),
        this.backend.environments(),
        this.backend.dirty(),
      ]);
      this.trees[id] = tree;
      const remembered = loadSetting<string | null>(`env:${id}`, null, isName);
      this.views[id] = {
        active: null,
        drafts: {},
        dirty,
        environments,
        environment: environments.some((e) => e.name === remembered) ? remembered : (environments[0]?.name ?? null),
        results: {},
        sending: {},
      };
    } else {
      if (!this.trees[id]) this.trees[id] = await this.backend.tree();
      // Tabs whose request was dropped by a reload are read again.
      for (const t of this.openTabs) {
        if (t.collection !== id || t.path in known.drafts) continue;
        try {
          known.drafts[t.path] = await this.backend.getRequest(t.path);
        } catch {
          // Gone in this version of the files.
        }
      }
      this.openTabs = this.openTabs.filter((t) => t.collection !== id || t.path in known.drafts);
      if (known.active && !(known.active in known.drafts)) known.active = null;
      known.active ??= this.openTabs.find((t) => t.collection === id)?.path ?? null;
    }
    this.collection = info;
    this.loadCount++;
  }

  /** Wave 14: opens a request of any collection in the sidebar in a tab. */
  async selectRef(ref: RequestRef): Promise<void> {
    await this.activate(ref.collection);
    if (this.collection?.id === ref.collection) await this.select(ref.path);
  }

  /** Wave 14: the draft of a request of any collection that was shown (tab labels). */
  draftOf(ref: RequestRef): RequestSpec | undefined {
    return this.views[ref.collection]?.drafts[ref.path];
  }

  /** Wave 14: whether a request of any collection has unsaved changes. */
  isDirty(ref: RequestRef): boolean {
    return this.views[ref.collection]?.dirty.includes(ref.path) ?? false;
  }

  /** Wave 14: closes a tab of any collection. Closing the active tab shows its neighbour in the
   *  tab bar, which may belong to another collection (that one becomes active). */
  async closeRef(ref: RequestRef): Promise<void> {
    const i = this.openTabs.findIndex((t) => t.collection === ref.collection && t.path === ref.path);
    if (i < 0) return;
    const wasShown = ref.collection === this.collection?.id && this.active === ref.path;
    this.openTabs = this.openTabs.filter((_, k) => k !== i);
    const view = this.views[ref.collection];
    if (view && view.active === ref.path) {
      const same = this.openTabs.filter((t) => t.collection === ref.collection);
      view.active = same.find((t) => this.openTabs.indexOf(t) >= i)?.path ?? same.at(-1)?.path ?? null;
    }
    if (!wasShown || this.active) return;
    const next = this.openTabs[Math.min(i, this.openTabs.length - 1)];
    if (next) await this.selectRef(next);
  }

  /** Tab bar: the list of all open tabs ("Go to tab…") is open. */
  tabList = $state(false);

  /** Index of the shown tab in `openTabs`, `-1` for none. */
  get activeTabIndex(): number {
    return this.openTabs.findIndex((t) => t.collection === this.activeCollection && t.path === this.active);
  }

  /** Tab bar: the next (`1`) or previous (`-1`) tab, wrapping. */
  async stepTab(delta: 1 | -1): Promise<void> {
    const n = this.openTabs.length;
    if (n === 0) return;
    const i = this.activeTabIndex;
    await this.selectRef(this.openTabs[i < 0 ? 0 : (i + delta + n) % n]);
  }

  /** Tab bar: tab `n` (1-based); `last` jumps to the final tab (Mod+9). */
  async gotoTab(n: number, last = false): Promise<void> {
    const ref = last ? this.openTabs.at(-1) : this.openTabs[n - 1];
    if (ref) await this.selectRef(ref);
  }

  /** Tab bar: closes several tabs. Unsaved changes stay in their drafts, as with `closeRef`
   *  (reopening the request shows them). The shown tab goes last, so its neighbour that
   *  stays is shown afterwards. */
  async closeRefs(refs: readonly RequestRef[]): Promise<void> {
    const shown = (r: RequestRef) => r.collection === this.activeCollection && r.path === this.active;
    for (const r of [...refs].sort((a, b) => Number(shown(a)) - Number(shown(b)))) await this.closeRef(r);
  }

  /** Wave 14: reads the tree of collection `id` if it was not yet (its sidebar node opens). */
  async ensureTree(id: string): Promise<void> {
    if (this.trees[id]) return;
    await this.guard(async () => {
      const tree = await this.backend.collections.tree(id);
      this.trees[id] = tree;
    });
  }

  /** Wave 14: "Add collection…": a folder picker, its collections join the sidebar. */
  async addCollection(): Promise<void> {
    await this.guard(async () => {
      const set = await this.backend.collections.add();
      if (!set) return;
      this.applySet(set);
      if (set.active && set.active !== this.collection?.id) await this.activate(set.active);
    });
  }

  // ---- wave 15: repositories in the set ----

  /** Wave 15: makes repository `id` the one every Git view acts on (a choice in the sidebar;
   *  activating a collection brings Git back to that collection's repository). */
  async activateRepository(id: string): Promise<void> {
    if (id === this.activeRepository) return;
    await this.guard(() =>
      this.enqueue(async () => {
        await this.backend.repositories.activate(id);
        await this.refreshSet();
      }),
    );
  }

  /** Wave 15: "Open repository": a folder picker; the folder joins the sidebar with its
   *  collections, or as a repository root if it holds none. */
  async openRepository(): Promise<void> {
    await this.guard(async () => {
      const set = await this.backend.repositories.open();
      if (!set) return;
      this.applySet(set);
      if (set.active && set.active !== this.collection?.id) await this.activate(set.active);
    });
  }

  /** Wave 15: "Init repository": a folder picker, `git init`, and the folder joins the
   *  sidebar as a repository root. */
  async initRepository(): Promise<void> {
    const entry = await this.guard(() => this.backend.repositories.init());
    if (entry) await this.adoptRepository(entry);
  }

  /** Wave 15: shows a repository the backend has just added (clone, init): its first
   *  collection becomes active (the other collections and their tabs stay), or, without any,
   *  the sidebar lists it as a repository root. */
  async adoptRepository(entry: RepositoryEntry): Promise<void> {
    const set = await this.guard(() => this.backend.collections.current());
    if (!set) return;
    const first = set.collections.find((c) => entry.collections.includes(c.id));
    if (first && first.id !== this.collection?.id) {
      await this.adoptAdded({ id: first.id, name: first.name, location: first.location });
      return;
    }
    await this.guard(() => this.enqueue(() => this.refreshSet()));
  }

  /** Wave 14: takes a folder and its collections out of the sidebar (files stay); refused
   *  while one of them has unsaved changes. */
  async removeRoot(root: string): Promise<void> {
    await this.guard(() =>
      this.enqueue(async () => {
        const set = await this.backend.collections.removeRoot(root);
        const gone = new Set(this.roots.find((r) => r.id === root)?.collections ?? []);
        this.openTabs = this.openTabs.filter((t) => !gone.has(t.collection));
        for (const id of gone) {
          delete this.views[id];
          delete this.trees[id];
        }
        this.applySet(set);
        if (!set.active) {
          this.collection = null;
          this.backendActive = null;
          this.loadCount++;
        } else if (set.active !== this.collection?.id || set.active !== this.backendActive) {
          await this.show_(await this.backendTo(set.active));
        }
      }),
    );
  }

  /** Wave 14: creates a collection (unsaved) and makes it active; `null` if cancelled. */
  async createCollection(spec: NewCollection): Promise<CollectionInfo | null> {
    return (
      (await this.guard(() =>
        this.enqueue(async () => {
          const info = await this.backend.collections.create(spec);
          if (!info) return null;
          if (this.collections.length === 0) {
            await this.load(info);
            return info;
          }
          this.backendActive = info.id;
          await this.show_(info);
          await this.refreshSet();
          return info;
        }),
      )) ?? null
    );
  }

  /** Re-reads the set (names, roots, unsaved counts) from the backend. */
  private async refreshSet(): Promise<void> {
    this.applySet(await this.backend.collections.current());
  }

  private applySet(set: CollectionSet): void {
    this.roots = set.roots;
    this.collections = set.collections;
    this.repositories = set.repositories ?? [];
    this.activeRepository = set.active_repository ?? null;
  }

  private setDirty(id: string, dirty: string[]): void {
    const view = this.views[id];
    if (view) view.dirty = dirty;
    const i = this.collections.findIndex((c) => c.id === id);
    if (i >= 0 && this.collections[i].dirty !== dirty.length) this.collections[i].dirty = dirty.length;
  }

  /** Wave 14: asks the shell to show a view of the main area. */
  show(view: MainView): void {
    this.view = { view, seq: ++this.seq };
  }

  /** Wave 14: shows a collection tab (`<extension id>/<tab id>`) of the active collection. */
  openCollectionTab(key: string): void {
    this.show({ kind: "collection-tab", key });
  }

  /** Wave 14: shows a request tab of the active request: `<extension id>/<tab id>` for an
   *  extension's tab, or a plugin's `<plugin id>/<tab id>`. Taken as soon as the request editor
   *  offers that tab for the active request; a later request replaces it. */
  openRequestTab(key: string): void {
    this.requestTab = { key, seq: ++this.seq };
  }

  /** Wave 17: opens a shared request (a link, a fragment or an `rt:` payload; `ShareApi.open`)
   *  as an unsaved draft in the active collection or, without one, in a new empty session.
   *  Nothing runs and nothing is sent. Strand C fills it in; until then it rejects with kind
   *  `unsupported`. */
  async openShared(input: string): Promise<void> {
    void input;
    throw new BackendError("unsupported", "Opening a shared request is not implemented yet (wave 17, strand C).");
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

  /** Wave 14: shows a collection the backend has just added to the set (an import as a new
   *  collection) and makes it active; the other collections and their tabs stay. */
  async adoptAdded(info: CollectionInfo): Promise<void> {
    await this.guard(() =>
      this.enqueue(async () => {
        if (this.collections.length === 0) return this.load(info);
        this.backendActive = info.id;
        await this.show_(info);
        await this.refreshSet();
      }),
    );
  }

  /** A collection set the backend has just opened (open, recent, import, clone): everything
   *  shown before is replaced. */
  private async load(info: CollectionInfo): Promise<void> {
    this.openTabs = [];
    this.views = {};
    this.trees = {};
    this.collection = null;
    this.backendActive = info.id;
    await this.show_(info);
    await this.refreshSet();
  }

  /** Re-reads what changed underneath (Git checkout, pull, conflict resolution): every
   *  collection of the active collection's repository (wave 14). Keeps the tabs whose requests
   *  still exist, with their state from the files; the active environment stays if it still
   *  exists. Collections of the repository that are not active are re-read when shown. */
  async reload(): Promise<void> {
    if (!this.collection) return;
    await this.guard(() =>
      this.enqueue(async () => {
        const id = this.collection!.id;
        const before = this.collections.find((c) => c.id === id);
        await this.refreshSet();
        const mates = this.collections.filter((c) =>
          before?.repo ? c.repo === before.repo : c.id === id,
        );
        for (const mate of mates) {
          if (mate.id === id) continue;
          // Read again when shown or opened in the sidebar.
          const view = this.views[mate.id];
          if (view) {
            view.drafts = {};
            view.dirty = await this.backend.collections.dirty(mate.id);
          }
          if (this.trees[mate.id]) this.trees[mate.id] = await this.backend.collections.tree(mate.id);
        }
        // The views of collections that left the set go.
        const present = new Set(this.collections.map((c) => c.id));
        this.openTabs = this.openTabs.filter((t) => present.has(t.collection));
        if (!present.has(id)) {
          const next = this.collections[0];
          if (next) await this.show_(await this.backendTo(next.id));
          return;
        }
        this.tree = await this.backend.tree();
        const view = this.current!;
        const drafts: Record<string, RequestSpec> = {};
        for (const path of this.tabs) {
          try {
            drafts[path] = await this.backend.getRequest(path);
          } catch {
            // The request is gone in this version of the files.
          }
        }
        view.drafts = drafts;
        this.openTabs = this.openTabs.filter((t) => t.collection !== id || t.path in drafts);
        if (view.active && !(view.active in drafts)) view.active = this.tabs[0] ?? null;
        view.environments = await this.backend.environments();
        if (!view.environments.some((e) => e.name === view.environment)) {
          view.environment = view.environments[0]?.name ?? null;
        }
        this.setDirty(id, await this.backend.dirty());
        this.loadCount++;
      }),
    );
  }

  /** Opens a request of the active collection in a tab (or focuses its tab). */
  async select(path: string): Promise<void> {
    const id = this.collection?.id;
    const view = this.current;
    if (!id || !view) return;
    await this.guard(async () => {
      if (!view.drafts[path]) view.drafts[path] = await this.backend.getRequest(path);
      if (!this.openTabs.some((t) => t.collection === id && t.path === path)) {
        this.openTabs = [...this.openTabs, { collection: id, path }];
      }
      view.active = path;
    });
  }

  /** Closes a tab of the active collection (`closeRef` for any). */
  close(path: string): void {
    const id = this.collection?.id;
    if (id) void this.closeRef({ collection: id, path });
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
    const id = this.collection?.id;
    if (!id) return;
    await this.enqueue(() =>
      this.guard(async () => {
        // The edit belongs to the collection that was active when it was made.
        if (this.backendActive !== id) await this.backendTo(id);
        await this.backend.updateRequest(path, snapshot);
        this.setDirty(id, await this.backend.dirty());
        if (this.trees[id]) this.trees[id] = await this.backend.tree();
        const shown = this.collection?.id;
        if (shown && shown !== id) await this.backendTo(shown);
      }),
    );
  }

  /** Resolves when every `edit` so far has reached the backend (wave 8: sessions open the
   *  request as edited, like `send`). */
  async settled(): Promise<void> {
    await this.lastEdit;
  }

  /** Re-reads the unsaved files of the active collection, after a change that did not go through
   *  `edit` (staged companion files, folder scripts), so "Save" and the dirty marks follow. */
  async refreshDirty(): Promise<void> {
    await this.lastEdit;
    const id = this.collection?.id;
    if (!id) return;
    this.setDirty(id, await this.backend.dirty());
  }

  /** A new request named `name` in `folder` of the active collection (unsaved), opened. */
  async create(folder: string, name: string): Promise<void> {
    await this.lastEdit;
    await this.guard(async () => {
      const path = await this.backend.createRequest(folder, name);
      this.tree = await this.backend.tree();
      this.dirty = await this.backend.dirty();
      await this.select(path);
    });
  }

  /** Saves every collection with unsaved changes (wave 14: Mod+S saves all). */
  async save(): Promise<void> {
    await this.guard(() =>
      this.enqueue(async () => {
        const saved = await this.backend.collections.saveAll();
        for (const c of saved) {
          if (this.views[c.collection]) this.setDirty(c.collection, await this.backend.collections.dirty(c.collection));
        }
        if (this.collection) this.setDirty(this.collection.id, await this.backend.dirty());
      }),
    );
  }

  /** Sends a request of the active collection; its result stays with that collection, also
   *  when another one becomes active meanwhile. */
  async send(path: string | null = this.active): Promise<void> {
    const view = this.current;
    const collectionId = this.collection?.id;
    if (!path || !view || !collectionId) return;
    view.sending[path] = true;
    await this.lastEdit;
    if (this.sendConfirm && !(await this.sendConfirm(path).catch(() => true))) {
      view.sending[path] = false;
      return;
    }
    if (this.sendGate && !(await this.sendGate(path).catch(() => true))) {
      this.sending[path] = false;
      return;
    }
    await this.guard(async () => {
      const previous = view.results[path];
      const result = await this.backend.send(path, { environment: view.environment, vars: [] });
      view.results[path] = result;
      if (previous?.response_id != null) await this.backend.releaseResponse(previous.response_id);
      await this.announceSent(collectionId, view, path, result);
    });
    view.sending[path] = false;
    this.sendCount++;
  }

  /** Wave 17: calls `listener` after every send of the app, in every collection, with the
   *  result and the request as it was sent (variables not resolved). Not called when the
   *  collection switched observers off (`extensions.requesttailor.observe`). Listener errors
   *  are logged. Returns the unsubscribe; the shell connects `AppExtension.onExchange` here. */
  onSent(listener: (sent: SentExchange) => void): () => void {
    this.sentListeners.add(listener);
    return () => this.sentListeners.delete(listener);
  }

  private async announceSent(collection: string, view: CollectionView, path: string, result: StoredResponse): Promise<void> {
    if (this.sentListeners.size === 0 || result.observe === false) return;
    // The request as the send used it: the edits are flushed (`lastEdit`) before the send. Ask
    // the backend only while the sending collection still is the active one.
    const draft = view.drafts[path];
    const request = draft
      ? ($state.snapshot(draft) as RequestSpec)
      : this.collection?.id === collection
        ? await this.backend.getRequest(path).catch(() => null)
        : null;
    if (!request) return;
    const sent: SentExchange = { collection, path, response: result, request, variables: result.report.variables };
    for (const listener of this.sentListeners) {
      try {
        listener(sent);
      } catch (e) {
        console.warn("RequestTailor: a send listener failed:", e);
      }
    }
  }

  /** Selects the active collection's environment (remembered per collection and device). */
  setEnvironment(name: string | null): void {
    this.environment = name;
    if (this.collection) storeSetting(`env:${this.collection.id}`, name ?? undefined);
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
