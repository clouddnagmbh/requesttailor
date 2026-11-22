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
  type Backend,
  type CollectionInfo,
  type CompanionStatus,
  type Environment,
  type FileChange,
  type GitApi,
  type ImportApi,
  type LineSlice,
  type Preview,
  type RecentCollection,
  type RequestSpec,
  type RunnerApi,
  type ScriptsApi,
  type SearchHit,
  type SecretsApi,
  type SendInput,
  type StoredResponse,
  type TransportApi,
  type TransportMode,
  type TreeNode,
} from "@requesttailor/ui";

import { webAuth, type WebAuthApi } from "./auth";
import { createCompanionClient, type CompanionClient } from "./companion/client";
import { webCookies, type WebCookieApi } from "./cookies";
import { webRunner } from "./runner";
import { webStreams } from "./streams";
import { webGrpc } from "./grpc";
import { webGraphQl } from "./graphql";
import { webAgents } from "./agents";
import { webMcp } from "./mcp-client";
import { webAi } from "./ai";
import { webScriptTrust, type WebScriptTrust } from "./scripts";
import { SessionStore, webSessionVars } from "./session-vars";
import { webGit } from "./git";
import { WebHistory } from "./history";
import { webImports } from "./import";
import { webPlugins, type WebPluginApi } from "./plugins";
import { sendServices } from "./send-services";
import { webRecent, type WebRecent } from "./recent";
import { readDirectory, writeChanges } from "./fs/directory";
import { lastFolder, rememberFolder } from "./fs/recent";
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
}

const DEFAULT_DEPS: WebBackendDeps = {
  loadCore,
  openSecrets: openBrowserSecretStore,
  companion: () => createCompanionClient(),
};

export const UNSUPPORTED_MESSAGE =
  "This browser cannot open local folders. Use a current Chrome or Edge, or the RequestTailor desktop app.";

const NO_COMPANION_HOWTO = "Run `rt agent` on this computer to send requests the browser blocks.";

export interface Session {
  id: string;
  handle: FileSystemDirectoryHandle;
  ws: CoreWorkspace;
  secrets: Promise<SecretsApi> | null;
}

/** What the per-feature parts (./git, ./import, ./history, ./recent; wave 4) may use. */
export interface OpenOptions {
  /** Keep the collection that is open by the time the folder is read (start-up reopen). */
  unlessOpen?: boolean;
}

export interface WebHost {
  readonly deps: WebBackendDeps;
  /** The open collection; throws `BackendError` if none. */
  session(): Session;
  /** Opens `handle` as the collection (asks for read/write access if needed). */
  openHandle(handle: FileSystemDirectoryHandle, options?: OpenOptions): Promise<CollectionInfo>;
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
  readonly secrets: SecretsApi;
  readonly transport: TransportApi;

  readonly git: GitApi;
  readonly imports: ImportApi;
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
  readonly streams = webStreams();
  readonly grpc = webGrpc();
  readonly graphql = webGraphQl();
  readonly agents = webAgents(() => this.session());
  readonly mcp = webMcp();
  readonly ai = webAi();
  /** Wave 7: session variables per collection and environment (`session-vars.ts`). */
  private readonly sessionVars = new SessionStore();
  private readonly recent: WebRecent;

  private current: Session | null = null;
  private triedLast = false;
  private companionOn = false;
  private companionClient: CompanionClient | null = null;
  /** External secrets read through the companion, cached for the session (wave 5). */
  private readonly external: ExternalSecretReader;

  constructor(readonly deps: WebBackendDeps = DEFAULT_DEPS) {
    const host: WebHost = {
      deps,
      session: () => this.session(),
      openHandle: (handle, options) => this.openHandle(handle, options),
      companionCredentials: async () => {
        try {
          return await this.client().credentials();
        } catch {
          return null;
        }
      },
    };
    this.external = new ExternalSecretReader(host.companionCredentials);
    this.git = webGit(host);
    this.imports = webImports(host);
    this.history = new WebHistory(host);
    this.plugins = webPlugins(host);
    this.auth = webAuth(host);
    this.cookies = webCookies(host);
    this.scriptTrust = webScriptTrust(host);
    this.scripts = { ...this.scriptTrust, ...webSessionVars(host, this.sessionVars) };
    this.runner = webRunner({
      session: host.session,
      loadCore: deps.loadCore,
      // Each request of a run goes out like `send`: secret values, companion, services.
      sendContext: async (input) => {
        const s = this.session();
        return {
          companion: this.companionOn ? await this.companionCredentials() : null,
          services: await sendServices(this, s.id, input.environment),
          input: await this.coreInput(s, input, true),
        };
      },
    });
    this.recent = webRecent(host);
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
    if (!this.current) throw new BackendError("shell", "No collection is open.");
    return this.current;
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
        const open = this.current;
        if (open && (await open.handle.isSameEntry(handle).catch(() => false))) {
          return { id: open.id, name: open.ws.name().trim() || open.handle.name, location: open.handle.name };
        }
      }
    }
    if (!handle) {
      try {
        handle = await window.showDirectoryPicker({ id: "requesttailor-collection", mode: "readwrite", startIn });
      } catch (e) {
        if (isAbort(e)) return null;
        throw toBackendError(e);
      }
    }
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

  private openHandleNow(handle: FileSystemDirectoryHandle, options: OpenOptions): Promise<CollectionInfo> {
    return guard(async () => {
      await ensureReadWrite(handle);
      const [core, files] = await Promise.all([this.deps.loadCore(), readDirectory(handle)]);
      const ws = core.Workspace.open(files);
      const open = this.current;
      if (options.unlessOpen && open) {
        // A collection was opened meanwhile (the start-up reopen came late): keep it.
        ws.free();
        return { id: open.id, name: open.ws.name().trim() || open.handle.name, location: open.handle.name };
      }
      const id = await rememberFolder(handle);
      // The previous workspace is not freed here: a send or history write that started before
      // may still hold it (freeing it made them fail with "null pointer passed to rust").
      // wasm-bindgen's FinalizationRegistry frees it once nothing refers to it any more.
      this.current = { id, handle, ws, secrets: null };
      const info = { id, name: ws.name().trim() || handle.name, location: handle.name };
      this.recent.opened(info, handle);
      return info;
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
    return guard(async () => this.session().ws.dirty());
  }

  /** Writes all changes; the workspace forgets them only after every file was written. */
  save(): Promise<FileChange[]> {
    return guard(async () => {
      const s = this.session();
      const prepared = s.ws.prepareSave();
      try {
        const changes = prepared.changes;
        if (changes.length > 0) {
          await ensureReadWrite(s.handle);
          await writeChanges(s.handle, changes);
        }
        s.ws.commit(prepared);
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
