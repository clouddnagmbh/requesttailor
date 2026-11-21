// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The browser backend: rt-wasm (rt-app's Workspace and ResponseStore) for everything the core
// does, the File System Access API for the folder (./fs/directory.ts), the encrypted browser
// secret store for secret values (./secrets/store.ts), and the companion client for requests
// the browser blocks (./companion/client.ts). Owner: wave 3, strand E.
//
// - Nothing heavy happens at start: the core (WebAssembly), the secret store and the companion
//   client are loaded on first use, so the app starts fast and `?backend=mock` never needs them.
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
  type LineSlice,
  type Preview,
  type RequestSpec,
  type SecretsApi,
  type SendInput,
  type StoredResponse,
  type TransportApi,
  type TransportMode,
  type TreeNode,
} from "@requesttailor/ui";

import { createCompanionClient, type CompanionClient } from "./companion/client";
import { readDirectory, writeChanges } from "./fs/directory";
import { lastFolder, rememberFolder } from "./fs/recent";
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

interface Session {
  id: string;
  handle: FileSystemDirectoryHandle;
  ws: CoreWorkspace;
  secrets: Promise<SecretsApi> | null;
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

  private current: Session | null = null;
  private triedLast = false;
  private companionOn = false;
  private companionClient: CompanionClient | null = null;

  constructor(private readonly deps: WebBackendDeps = DEFAULT_DEPS) {
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
    return guard(async () => {
      await ensureReadWrite(handle);
      const [core, files] = await Promise.all([this.deps.loadCore(), readDirectory(handle)]);
      const ws = core.Workspace.open(files);
      const id = await rememberFolder(handle);
      this.current?.ws.free();
      this.current = { id, handle, ws, secrets: null };
      return { id, name: ws.name().trim() || handle.name, location: handle.name };
    });
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

  /** Values of the enabled secret variables of the selected environment. The secret store
   *  is only opened when that environment has secret variables. */
  private async secretValues(s: Session, environment: string | null): Promise<[string, string][]> {
    const env = environment == null ? undefined : s.ws.environments().find((e) => e.name === environment);
    const names = env?.variables.filter((v) => v.secret && v.enabled).map((v) => v.name) ?? [];
    if (!env || names.length === 0) return [];
    const store = await this.secretStore(s);
    const values: [string, string][] = [];
    for (const name of names) {
      const value = await store.get(env.name, name);
      if (value != null) values.push([name, value]);
    }
    return values;
  }

  private async coreInput(s: Session, input: SendInput): Promise<CoreSendInput> {
    return {
      environment: input.environment,
      vars: input.vars,
      secrets: await this.secretValues(s, input.environment),
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
      return await s.ws.send(path, await this.coreInput(s, input), companion);
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
