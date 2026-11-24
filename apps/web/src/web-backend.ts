// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The browser backend: rt-wasm (rt-app's Workspace and ResponseStore) for everything the core
// does, the File System Access API for the folder (./fs/directory.ts), the encrypted browser
// secret store for secret values (./secrets/store.ts), and the companion client for requests
// the browser blocks (./companion/client.ts). Owner: wave 3, strand E.
//
// - Nothing heavy happens at start: the core (WebAssembly) and the secret store are loaded on
//   first use, so the app starts fast and `?backend=mock` never needs them. The companion
//   client is created at start (cheap) so a pairing link in the URL is handled immediately.
// - The first "Open folder" of a page reopens the last folder without a picker if the browser
//   still grants access to it (Chrome's "allow on every visit"); otherwise the picker opens in
//   that folder.
// - Browsers without the File System Access API (Firefox, Safari) get a clear error message
//   on "Open folder" instead of a crash.

import {
  BackendError,
  type AiApi,
  type Backend,
  type CollectionInfo,
  type CompanionStatus,
  type CompareApi,
  type ContractApi,
  type DiffApi,
  type Environment,
  type FileChange,
  type GitApi,
  type ImportApi,
  type OpenApiApi,
  type LineSlice,
  type McpClientApi,
  type MergeApi,
  type Preview,
  type RecentCollection,
  type RequestSpec,
  type RescueApi,
  type CompanionApi,
  type FolderScriptsApi,
  type ShareApi,
  type SnapshotApi,
  type EnvCompareApi,
  type InspectApi,
  type PagingApi,
  type InboxApi,
  type LintApi,
  type ViewsApi,
  type CatalogueApi,
  type VariablesApi,
  type ReviewApi,
  type RunnerApi,
  type StreamsApi,
  type ScriptsApi,
  type SearchHit,
  type SecretsApi,
  type SendInput,
  type StoredResponse,
  type TimelineApi,
  type TransportApi,
  type TransportMode,
  type TreeNode,
  type RepositoryApi,
} from "@requesttailor/ui";

import { webAuth, type WebAuthApi } from "./auth";
import { createCompanionClient, type CompanionClient } from "./companion/client";
import { webCookies, type WebCookieApi } from "./cookies";
import { webRunner, type RunContext } from "./runner";
import { webStreams } from "./streams";
import { webGrpc } from "./grpc";
import { webGraphQl } from "./graphql";
import { webAgents } from "./agents";
import { webMcp } from "./mcp-client";
import { webAi, type AiCore } from "./ai";
// Wave 12: one file per strand (./wave12.ts).
import { webCompare } from "./compare";
import { webContract } from "./contract";
import { webOpenApi } from "./openapi";
import { webDiffs } from "./diffs";
import { webMerges } from "./merges";
import { webRescue } from "./rescue";
import { webCompanions } from "./companions";
import { webFolderScripts } from "./folder-scripts";
import { webShare, type CoreShare } from "./share";
import { webSnapshots } from "./snapshots";
import { webEnvCompare } from "./envcompare";
import { webInspect } from "./inspect";
import { webPaging } from "./paging";
import { webInbox } from "./inbox";
import { webLint } from "./lint";
import { webViews } from "./views";
import { webCatalogue } from "./catalogue";
import { webVariables } from "./variables";
import { webReview } from "./review";
import { webTimeline } from "./timeline";
import type { Wave12Host } from "./wave12";
import { webScriptTrust, type WebScriptTrust } from "./scripts";
import { SessionStore, webSessionVars } from "./session-vars";
import { webGit } from "./git";
import { webRepositories } from "./repositories";
import { WebHistory } from "./history";
import { webImports, type WebImports } from "./import";
import { webPlugins, type WebPluginApi } from "./plugins";
import { sendServices, type SendGuardFn } from "./send-services";
import { webRecent, type WebRecent } from "./recent";
import { WebCollectionSet, type Core as CollectionsCoreModule, type WebSession } from "./collections";
import { writeChanges } from "./fs/directory";
import { folderById, lastFolder, rememberFolder, rememberRoots, rememberedRoots } from "./fs/recent";
import { ExternalSecretReader } from "./secrets/external";
import { openBrowserSecretStore } from "./secrets/store";
import {
  loadCore,
  toBackendError,
  type CompanionCredentials,
  type CoreModule,
  type CoreSendInput,
  type CoreWorkspace,
} from "./wasm";

/** Replaceable parts, for tests. Defaults are the real implementations. */
export interface WebBackendDeps {
  loadCore: () => Promise<CoreModule>;
  openSecrets: (collectionId: string) => Promise<SecretsApi>;
  companion: () => CompanionClient;
  /** Wave 10: a distribution's send guard (`rt_core::guard`), asked before every send of
   *  this backend (send, runner, streams, gRPC, GraphQL). None in the builds of this repo. */
  sendGuard?: SendGuardFn;
}

const DEFAULT_DEPS: WebBackendDeps = {
  loadCore,
  openSecrets: openBrowserSecretStore,
  companion: () => createCompanionClient(),
};

export const UNSUPPORTED_MESSAGE =
  "This browser cannot open local folders. Use a current Chrome or Edge, or the RequestTailor desktop app.";

const NO_COMPANION_HOWTO = "Run `rt agent` on this computer to send requests the browser blocks.";

/** One open collection (wave 14: of the collection set, `./collections.ts`). */
export type Session = WebSession;

/** What the per-feature parts (./git, ./import, ./history, ./recent; wave 4) may use. */
export interface OpenOptions {
  /** Keep the collection that is open by the time the folder is read (start-up reopen). */
  unlessOpen?: boolean;
}

export interface WebHost {
  readonly deps: WebBackendDeps;
  /** The open (active) collection; throws `BackendError` if none. */
  session(): Session;
  /** Opens `handle` as the only root of the collection set (asks for read/write access if
   *  needed). Wave 14: a handle of a root or collection already in the set reads the
   *  collections of the active collection's repository again instead (after Git changed
   *  files). */
  openHandle(handle: FileSystemDirectoryHandle, options?: OpenOptions): Promise<CollectionInfo>;
  /** Wave 14: adds `handle` to the collection set as another root and makes its first
   *  collection active (the only root if nothing is open): an import as a new collection. */
  addHandle?(handle: FileSystemDirectoryHandle): Promise<CollectionInfo>;
  /** Wave 14: opens the recent root `id`: the whole remembered set if it is one of its roots,
   *  else `handle` alone. */
  openRecentRoot?(id: string, handle: FileSystemDirectoryHandle, interactive: boolean): Promise<CollectionInfo | null>;
  /** Wave 14: the Git repository of the active collection (its folder and the collection's
   *  folder in it, `""` at its root); `null` outside Git. Without it, the web Git takes the
   *  collection's folder as the repository. */
  repository?(): { handle: FileSystemDirectoryHandle; scope: string } | null;
  /** Wave 15: whether a collection of the active repository has unsaved changes (the active
   *  repository may be a repository root without any collection). */
  repositoryDirty?(): boolean;
  /** Wave 15: the id of the active repository (`CollectionSet.active_repository`), the one
   *  extensions' Git actions name; `null` when there is none. */
  activeRepository?(): string | null;
  /** Wave 15: reads the collections of the active repository again after Git changed files
   *  (there may be none). Without it, the active collection's folder is opened again. */
  reloadSet?(): Promise<void>;
  /** The companion's URL and token if it is paired, else `null`. */
  companionCredentials(): Promise<CompanionCredentials | null>;
}

function guardSync<T>(f: () => T): T {
  try {
    return f();
  } catch (e) {
    throw toBackendError(e);
  }
}

async function guard<T>(f: () => Promise<T>): Promise<T> {
  try {
    return await f();
  } catch (e) {
    throw toBackendError(e);
  }
}

function isAbort(e: unknown): boolean {
  return e instanceof DOMException && e.name === "AbortError";
}

async function ensureReadWrite(handle: FileSystemDirectoryHandle): Promise<void> {
  if (!handle.queryPermission || !handle.requestPermission) return;
  if ((await handle.queryPermission({ mode: "readwrite" })) === "granted") return;
  if ((await handle.requestPermission({ mode: "readwrite" })) !== "granted") {
    throw new BackendError("shell", `Permission to edit the folder "${handle.name}" was not granted.`);
  }
}

export class WebBackend implements Backend {
  readonly kind = "web" as const;
  /** Wave 14: the collections in the sidebar (`collections.ts`). */
  readonly collections: WebCollectionSet;
  /** Wave 15: repositories in the set (`repositories.ts`). */
  readonly repositories: RepositoryApi;
  readonly secrets: SecretsApi;
  readonly transport: TransportApi;

  readonly git: GitApi;
  readonly imports: WebImports;
  readonly openapi: OpenApiApi;
  readonly history: WebHistory;
  readonly plugins: WebPluginApi;
  readonly auth: WebAuthApi;
  readonly cookies: WebCookieApi;
  /** Wave 7: trust (strand A) and session variables (strand C); runner (strand D). */
  readonly scripts: ScriptsApi;
  /** Wave 7: the trust part of `scripts`, read by every send (`send-services.ts`). */
  readonly scriptTrust: WebScriptTrust;
  readonly runner: RunnerApi;
  /** Wave 8 (one file per strand). */
  readonly streams: StreamsApi;
  readonly grpc = webGrpc({ session: () => this.session(), sendContext: (i) => this.sendContext(i) });
  readonly graphql = webGraphQl({ session: () => this.session(), sendContext: (i) => this.sendContext(i) });
  readonly agents = webAgents(() => this.session());
  readonly mcp: McpClientApi;
  readonly ai: AiApi;
  /** Wave 12 (one file per strand). */
  readonly diffs: DiffApi;
  readonly merges: MergeApi;
  readonly compare: CompareApi;
  readonly timeline: TimelineApi;
  readonly review: ReviewApi;
  readonly contract: ContractApi;
  readonly rescue: RescueApi;
  /** Wave 13. */
  readonly companions: CompanionApi;
  /** Wave 14: variable completion. */
  readonly variables: VariablesApi;
  /** Wave 16: scripts of the collection and its folders (`folder-scripts.ts`). */
  readonly folderScripts: FolderScriptsApi;
  /** Wave 18: placeholders until the strands (one file each, `<name>.ts`). */
  readonly envCompare: EnvCompareApi;
  readonly inspect: InspectApi;
  readonly paging: PagingApi;
  readonly inbox: InboxApi;
  readonly lint: LintApi;
  readonly views: ViewsApi;
  readonly catalogue: CatalogueApi;
  /** Wave 17: snapshot tests (`snapshots.ts`) and sharing (`share.ts`). */
  readonly snapshots: SnapshotApi;
  readonly share: ShareApi;
  /** Wave 7: session variables per collection and environment (`session-vars.ts`). */
  private readonly sessionVars = new SessionStore();
  private readonly recent: WebRecent;

  private triedLast = false;
  private companionOn = false;
  private companionClient: CompanionClient | null = null;
  /** External secrets read through the companion, cached for the session (wave 5). */
  private readonly external: ExternalSecretReader;

  /** Wave 10: read by `sendServices` (`send-services.ts`). */
  get sendGuard(): SendGuardFn | undefined {
    return this.deps.sendGuard;
  }

  constructor(readonly deps: WebBackendDeps = DEFAULT_DEPS) {
    const host: WebHost = {
      deps,
      session: () => this.session(),
      openHandle: (handle, options) => this.openHandle(handle, options),
      addHandle: (handle) => this.addHandle(handle),
      openRecentRoot: (id, handle, interactive) => this.openRecentRoot(id, handle, interactive),
      repository: () => this.collections.repository(),
      repositoryDirty: () => this.collections.repositoryDirty(),
      activeRepository: () => this.collections.activeRepositoryId(),
      reloadSet: async () => {
        await this.collections.reloadSet();
      },
      companionCredentials: async () => {
        try {
          return await this.client().credentials();
        } catch {
          return null;
        }
      },
    };
    this.external = new ExternalSecretReader(host.companionCredentials);
    this.collections = new WebCollectionSet({
      loadCore: async () => (await deps.loadCore()) as CollectionsCoreModule,
      ensureReadWrite,
      rememberFolder,
      opened: (info, handle) => this.recent.opened(info, handle),
      remember: (roots, active) => void rememberRoots({ roots, active }),
      session: (id, handle, ws) => ({ id, handle, ws, secrets: null }),
      pick: (id) => this.pickFolder(id),
      saveActive: () => this.save(),
    });
    const git = webGit(host);
    this.git = git;
    this.repositories = webRepositories({
      collections: this.collections,
      pick: (id) => this.pickFolder(id),
      folders: git,
      checkout: (handle, branch) => git.checkoutAt(handle, branch),
    });
    this.imports = webImports(host);
    this.openapi = webOpenApi({
      host,
      imports: this.imports,
      // As for a send (external secrets read too), so what the gate finds is what goes out.
      coreInput: (input) => this.coreInput(this.session(), input, true),
      companionOn: () => this.companionOn,
    });
    this.history = new WebHistory(host);
    this.plugins = webPlugins(host);
    this.auth = webAuth(host);
    this.cookies = webCookies(host);
    this.scriptTrust = webScriptTrust(host);
    this.scripts = { ...this.scriptTrust, ...webSessionVars(host, this.sessionVars) };
    // Each request of a run, and each session (wave 8), goes out like `send`: secret
    // values, companion, services.
    const sendContext = async (input: SendInput) => {
      const s = this.session();
      return {
        companion: this.companionOn ? await this.companionCredentials() : null,
        services: await sendServices(this, s.id, input.environment),
        input: await this.coreInput(s, input, true),
      };
    };
    this.runner = webRunner({
      session: host.session,
      loadCore: deps.loadCore,
      sendContext,
    });
    this.streams = webStreams({ session: host.session, sendContext });
    // Wave 8, strand D: MCP servers over HTTP, inputs and companion as for a send.
    this.mcp = webMcp({
      session: host.session,
      companion: async () => (this.companionOn ? await this.companionCredentials() : null),
      coreInput: (input) => this.coreInput(this.session(), input, true),
      secretValues: async () => {
        const s = this.session();
        const values: string[] = [];
        for (const env of s.ws.environments()) {
          for (const [, value] of await this.secretValues(s, env.name)) values.push(value);
        }
        return values;
      },
    });
    this.recent = webRecent(host);
    const wave12: Wave12Host = { host, git: this.git, sendContext };
    this.diffs = webDiffs(wave12);
    this.merges = webMerges(wave12);
    this.compare = webCompare(wave12);
    this.timeline = webTimeline(wave12);
    this.review = webReview(wave12);
    this.contract = webContract(wave12);
    this.rescue = webRescue(wave12);
    this.companions = webCompanions(() => this.session());
    this.folderScripts = webFolderScripts(() => this.session());
    this.envCompare = webEnvCompare();
    this.inspect = webInspect();
    this.paging = webPaging();
    this.inbox = webInbox();
    this.lint = webLint();
    this.views = webViews();
    this.catalogue = webCatalogue();
    this.snapshots = webSnapshots(
      () => this.session(),
      async (s, environment) => (await this.secretValues(s, environment)).map(([, value]) => value),
    );
    this.share = webShare(
      () => this.session(),
      async () => (await deps.loadCore()) as unknown as CoreShare,
    );
    this.variables = webVariables(
      () => this.session(),
      (s, input) => this.coreInput(s, input),
    );
    this.ai = webAi({
      session: host.session,
      openSecrets: deps.openSecrets,
      // Secret values and session variables as for a send, only for masking (wave 8).
      sendContext: async (input) => ({
        input: await this.coreInput(this.session(), input, true),
        companion: this.companionOn ? await this.companionCredentials() : null,
      }),
      core: async () => (await deps.loadCore()) as unknown as AiCore,
    });
    const store = () => this.secretStore(this.session());
    this.secrets = {
      has: (env, name) => guard(async () => (await store()).has(env, name)),
      get: (env, name) => guard(async () => (await store()).get(env, name)),
      set: (env, name, value) => guard(async () => (await store()).set(env, name, value)),
      delete: (env, name) => guard(async () => (await store()).delete(env, name)),
    };
    this.transport = {
      mode: (): TransportMode => (this.companionOn ? "companion" : "browser"),
      companion: () => this.companionStatus(),
      useCompanion: (enabled) => this.useCompanion(enabled),
    };
    // Created right away (it only reads storage): it pairs from a `#rt-pair=` code in the URL
    // as soon as the page opens, e.g. from the pairing link `rt agent` prints.
    try {
      this.companionClient = deps.companion();
    } catch (e) {
      console.warn("RequestTailor companion client unavailable:", e);
    }
  }

  private session(): Session {
    return this.collections.session();
  }

  /** The folder picker; `null` if cancelled. */
  private async pickFolder(id: string, startIn?: FileSystemDirectoryHandle): Promise<FileSystemDirectoryHandle | null> {
    if (typeof window.showDirectoryPicker !== "function") {
      throw new BackendError("unsupported", UNSUPPORTED_MESSAGE);
    }
    try {
      return await window.showDirectoryPicker({ id, mode: "readwrite", startIn });
    } catch (e) {
      if (isAbort(e)) return null;
      throw toBackendError(e);
    }
  }

  private secretStore(s: Session): Promise<SecretsApi> {
    s.secrets ??= this.deps.openSecrets(s.id).catch((e: unknown) => {
      s.secrets = null;
      throw e;
    });
    return s.secrets;
  }

  // ---- collection ----

  async openCollection(): Promise<CollectionInfo | null> {
    if (typeof window.showDirectoryPicker !== "function") {
      throw new BackendError("unsupported", UNSUPPORTED_MESSAGE);
    }
    let handle: FileSystemDirectoryHandle | null = null;
    let startIn: FileSystemDirectoryHandle | undefined;
    if (!this.triedLast) {
      this.triedLast = true;
      const last = await lastFolder();
      startIn = last?.handle;
      if (last && (await last.handle.queryPermission?.({ mode: "readwrite" }).catch(() => "denied")) === "granted") {
        handle = last.handle;
        // Already open (the start-up reopen got there first): keep it instead of replacing
        // the workspace under a send that may be running.
        if (this.collections.isOpen && (await this.collections.contains(handle))) {
          return this.collections.info(this.session().id);
        }
      }
    }
    handle ??= await this.pickFolder("requesttailor-collection", startIn);
    if (!handle) return null;
    return this.openHandle(handle);
  }

  /** The open in flight, so concurrent opens of the same folder (e.g. the start-up reopen
   *  and a click on Open folder) share one workspace instead of replacing each other while a
   *  send is running. */
  private opening: { handle: FileSystemDirectoryHandle; info: Promise<CollectionInfo> } | null = null;

  private async openHandle(handle: FileSystemDirectoryHandle, options: OpenOptions = {}): Promise<CollectionInfo> {
    const pending = this.opening;
    if (pending && (await pending.handle.isSameEntry(handle).catch(() => false))) return pending.info;
    const info = this.openHandleNow(handle, options);
    const entry = { handle, info };
    this.opening = entry;
    try {
      return await info;
    } finally {
      if (this.opening === entry) this.opening = null;
    }
  }

  private addHandle(handle: FileSystemDirectoryHandle): Promise<CollectionInfo> {
    if (!this.collections.isOpen) return this.openHandle(handle);
    return guard(async () => {
      await this.collections.addRoot(handle, true);
      const active = (await this.collections.current()).active;
      return active ? this.collections.activate(active) : this.collections.info(this.session().id);
    });
  }

  private openHandleNow(handle: FileSystemDirectoryHandle, options: OpenOptions): Promise<CollectionInfo> {
    return guard(async () => {
      const set = this.collections;
      if (set.isOpen && (await set.contains(handle))) {
        // Already in the sidebar: the start-up reopen keeps it; otherwise the files changed
        // underneath (Git) and the repository's collections are read again.
        return options.unlessOpen ? set.info(this.session().id) : set.reload();
      }
      // A collection opened meanwhile (the start-up reopen came late) is kept.
      const keep = () => !!options.unlessOpen && set.isOpen;
      const info = await set.openRoot(handle, keep);
      return info ?? set.info(this.session().id);
    });
  }

  private openRecentRoot(id: string, handle: FileSystemDirectoryHandle, interactive: boolean): Promise<CollectionInfo | null> {
    return guard(async () => {
      const saved = await rememberedRoots();
      if (saved) {
        const keep = () => !interactive && this.collections.isOpen;
        const folder = async (rootId: string) => {
          if (rootId === id) return handle;
          const f = await folderById(rootId);
          if (!f) return null;
          const granted = !f.handle.queryPermission || (await f.handle.queryPermission({ mode: "readwrite" }).catch(() => "denied")) === "granted";
          return granted ? f.handle : null;
        };
        const restored = await this.collections.restore(id, saved, folder, keep);
        if (restored) return restored;
        if (keep()) return this.collections.info(this.session().id);
      }
      return this.openHandle(handle, { unlessOpen: !interactive });
    });
  }

  recentCollections(): Promise<RecentCollection[]> {
    return guard(() => this.recent.list());
  }
  openRecent(id: string, interactive: boolean): Promise<CollectionInfo | null> {
    return guard(() => this.recent.open(id, interactive));
  }
  search(query: string, limit: number): Promise<SearchHit[]> {
    return guard(async () => this.session().ws.search(query, limit));
  }

  tree(): Promise<TreeNode> {
    return guard(async () => this.session().ws.tree());
  }
  getRequest(path: string): Promise<RequestSpec> {
    return guard(async () => this.session().ws.request(path));
  }
  createRequest(folder: string, name: string): Promise<string> {
    return guard(async () => this.session().ws.createRequest(folder, name));
  }
  createFolder(path: string): Promise<string> {
    return guard(async () => this.session().ws.createFolder(path));
  }
  updateRequest(path: string, spec: RequestSpec): Promise<void> {
    return guard(async () => this.session().ws.updateRequest(path, spec));
  }
  dirty(): Promise<string[]> {
    return guard(async () => this.collections.dirtyNow(this.session().id));
  }

  /** Writes all changes of the active collection; the workspace forgets them only after every
   *  file was written. A new collection's files (wave 14) go first. */
  save(): Promise<FileChange[]> {
    return guard(async () => {
      const s = this.session();
      const prepared = s.ws.prepareSave();
      try {
        const changes = [...prepared.changes];
        for (const f of s.created ?? []) if (!changes.some((c) => c.path === f.path)) changes.push(f);
        changes.sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
        if (changes.length > 0) {
          await ensureReadWrite(s.handle);
          // A spec staged at the repository root has a path with a leading `/` (wave 17); in
          // the browser the repository root is the collection folder.
          await writeChanges(
            s.handle,
            changes.map((c) => (c.path.startsWith("/") ? { ...c, path: c.path.replace(/^\/+/, "") } : c)),
          );
        }
        s.ws.commit(prepared);
        s.created = undefined;
        return changes;
      } finally {
        prepared.free();
      }
    });
  }

  environments(): Promise<Environment[]> {
    return guard(async () => this.session().ws.environments());
  }
  updateEnvironment(env: Environment): Promise<void> {
    return guard(async () => this.session().ws.updateEnvironment(env));
  }
  deleteEnvironment(name: string): Promise<void> {
    return guard(async () => this.session().ws.deleteEnvironment(name));
  }

  // ---- sending ----

  /** Values of the enabled secret variables of the selected environment, plus its external
   *  secrets (browser store values win). The secret store is only opened when that
   *  environment has secret variables. External secrets are read through the companion when
   *  sending (`read`); preview uses only those already read. */
  private async secretValues(s: Session, environment: string | null, read = false): Promise<[string, string][]> {
    const env = environment == null ? undefined : s.ws.environments().find((e) => e.name === environment);
    const names = env?.variables.filter((v) => v.secret && v.enabled).map((v) => v.name) ?? [];
    if (!env) return [];
    const values: [string, string][] = [];
    if (names.length > 0) {
      const store = await this.secretStore(s);
      for (const name of names) {
        const value = await store.get(env.name, name);
        if (value != null) values.push([name, value]);
      }
    }
    const external = env.external_secrets;
    if (external) {
      const skip = values.map(([name]) => name);
      values.push(...(read ? await this.external.resolve(external, skip) : this.external.cached(external, skip)));
    }
    return values;
  }

  /** Wave 8, strand B (`grpc.ts`, `graphql.ts`): what a send needs, as for the runner. */
  private async sendContext(input: SendInput): Promise<RunContext> {
    const s = this.session();
    return {
      companion: this.companionOn ? await this.companionCredentials() : null,
      services: await sendServices(this, s.id, input.environment),
      input: await this.coreInput(s, input, true),
    };
  }

  private async coreInput(s: Session, input: SendInput, read = false): Promise<CoreSendInput> {
    return {
      environment: input.environment,
      // Wave 7: the session's variables in the runtime layer, the caller's own win.
      vars: this.sessionVars.runtime(s.id, input.environment, input.vars),
      secrets: await this.secretValues(s, input.environment, read),
    };
  }

  preview(path: string, input: SendInput): Promise<Preview> {
    return guard(async () => {
      const s = this.session();
      return s.ws.preview(path, await this.coreInput(s, input));
    });
  }

  /** Transport errors (`blocked` when the browser refuses, e.g. CORS) are in
   *  `report.error`, from the core. */
  send(path: string, input: SendInput): Promise<StoredResponse> {
    return guard(async () => {
      const s = this.session();
      const companion = this.companionOn ? await this.companionCredentials() : null;
      const services = await sendServices(this, s.id, input.environment);
      const result = await s.ws.send(path, await this.coreInput(s, input, true), companion, services);
      this.sessionVars.keep(s.id, result.report);
      try {
        this.history.record(s.id, s.ws.request(path), result);
      } catch (e) {
        console.warn("RequestTailor: recording the send in the history failed:", e);
      }
      return result;
    });
  }

  responseLines(id: number, start: number, count: number): Promise<LineSlice> {
    return guard(async () => this.session().ws.responseLines(id, start, count));
  }
  responseBytes(id: number, offset: number, length: number): Promise<Uint8Array> {
    return guard(async () => this.session().ws.responseBytes(id, offset, length));
  }
  filterJson(id: number, expression: string): Promise<string> {
    return guard(async () => this.session().ws.filterJson(id, expression));
  }
  releaseResponse(id: number): Promise<void> {
    return guard(async () => this.session().ws.releaseResponse(id));
  }

  // ---- companion ----

  private client(): CompanionClient {
    this.companionClient ??= guardSync(() => this.deps.companion());
    return this.companionClient;
  }

  private async companionStatus(): Promise<CompanionStatus> {
    try {
      return await this.client().status();
    } catch {
      return { available: false, paired: false, url: null, howTo: NO_COMPANION_HOWTO };
    }
  }

  private async companionCredentials(): Promise<CompanionCredentials> {
    const credentials = await this.client().credentials();
    if (!credentials) {
      this.companionOn = false;
      throw new BackendError("companion", "The companion is no longer paired. Turn it on again to pair.");
    }
    return credentials;
  }

  private useCompanion(enabled: boolean): Promise<void> {
    return guard(async () => {
      if (!enabled) {
        this.companionOn = false;
        return;
      }
      const client = this.client();
      if (!(await client.credentials())) await client.pair();
      if (!(await client.credentials())) {
        throw new BackendError("companion", "Pairing with the companion did not complete.");
      }
      this.companionOn = true;
    });
  }
}

export async function createWebBackend(deps?: Partial<WebBackendDeps>): Promise<Backend> {
  return new WebBackend({ ...DEFAULT_DEPS, ...deps });
}
