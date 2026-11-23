// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `GitApi` of the web app: isomorphic-git over the collection's directory handle (./fs.ts,
// operations in ./repo.ts); network (push, pull, clone) through the companion (./http.ts),
// since browsers block Git hosts (CORS). Diffs come from the core (`textDiff`), so they match
// the desktop app. Owner: wave 4, strand A2. Wave 12 (interface): revisions (./revisions.ts).
//
// - The repository root is the collection folder itself: the browser grants no access above
//   the folder the user picked. Without `.git` there, `status` says "not a repository".
// - isomorphic-git (with its Buffer polyfill) loads on the first Git call, not at app start.
// - Operations run one at a time. `checkout`, `pull` and `resolve` refuse while the workspace
//   has unsaved changes and reopen the collection afterwards, so the core sees the new files.
// - `commit` runs the secret scanner (./scan.ts) over the files first and refuses with
//   `secrets-found` while they contain credentials (wave 5, strand E).
// - Credentials per host live in the encrypted browser secret store (collection
//   "git:credentials", environment = host, variables `username` and `password`). The author
//   identity is not secret: it is kept per browser profile in `localStorage`.

import {
  BackendError,
  type CollectionInfo,
  type GitApi,
  type GitIdentity,
  type SecretsApi,
  LOCAL_CLONE_UNSUPPORTED,
  looksLocal,
} from "@requesttailor/ui";

import type { WebHost } from "../web-backend";
import { directoryFs } from "./fs";
import { companionHttp, NO_COMPANION_MESSAGE } from "./http";
import type { Network, Repo } from "./repo";
import { refuseSecrets, scanPaths } from "./scan";
import { webGitActions } from "./actions";
import { webGitGraph, type WebGitContext } from "./graph";

type RepoModule = typeof import("./repo");
type RevisionsModule = typeof import("./revisions");

/** Secret-store collection id for Git credentials. */
export const CREDENTIALS_STORE = "git:credentials";
const IDENTITY_KEY = "requesttailor.git.identity";

let loading: Promise<RepoModule> | null = null;
function ops(): Promise<RepoModule> {
  loading ??= import("./repo").catch((e: unknown) => {
    loading = null;
    throw e;
  });
  return loading;
}

// Wave 12 (interface): revisions, loaded with isomorphic-git on first use.
let loadingRevisions: Promise<RevisionsModule> | null = null;
function revisions(): Promise<RevisionsModule> {
  loadingRevisions ??= import("./revisions").catch((e: unknown) => {
    loadingRevisions = null;
    throw e;
  });
  return loadingRevisions;
}

function repoAt(handle: FileSystemDirectoryHandle): Repo {
  return { fs: directoryFs(handle), dir: "/", cache: {} };
}

function isIdentity(v: unknown): v is GitIdentity {
  return (
    typeof v === "object" &&
    v !== null &&
    typeof (v as GitIdentity).name === "string" &&
    typeof (v as GitIdentity).email === "string"
  );
}

/** The identity, per browser profile; memory only if `localStorage` is unavailable. */
function identityStore() {
  let memory: GitIdentity | null = null;
  return {
    get(): GitIdentity | null {
      try {
        const raw = localStorage.getItem(IDENTITY_KEY);
        if (raw === null) return memory;
        const v: unknown = JSON.parse(raw);
        return isIdentity(v) ? v : memory;
      } catch {
        return memory;
      }
    },
    set(identity: GitIdentity): void {
      memory = identity;
      try {
        localStorage.setItem(IDENTITY_KEY, JSON.stringify(identity));
      } catch {
        // Memory only.
      }
    },
  };
}

function validIdentity(identity: GitIdentity): GitIdentity {
  const name = identity.name.trim();
  const email = identity.email.trim();
  if (!name) throw new BackendError("git", "Enter your name for commits.");
  if (!/^[^\s@<>]+@[^\s@<>]+$/.test(email)) throw new BackendError("git", "Enter a valid email address for commits.");
  return { name, email };
}

function isAbort(e: unknown): boolean {
  return e instanceof DOMException && e.name === "AbortError";
}

export function webGit(host: WebHost): GitApi {
  const identity = identityStore();
  let secrets: Promise<SecretsApi> | null = null;
  const store = () =>
    (secrets ??= host.deps.openSecrets(CREDENTIALS_STORE).catch((e: unknown) => {
      secrets = null;
      throw e;
    }));

  // One Git operation at a time (status refreshes included).
  let queue: Promise<unknown> = Promise.resolve();
  function run<T>(op: () => Promise<T>): Promise<T> {
    const next = queue.then(async () => {
      try {
        return await op();
      } catch (e) {
        throw (await ops()).gitError(e);
      }
    });
    queue = next.catch(() => undefined);
    return next;
  }

  const session = () => host.session();
  const repo = () => repoAt(session().handle);
  // Wave 14: what the graph (./graph.ts) and the wizards (./actions.ts) get.
  const wave14: WebGitContext = { host, repo, run: (op) => run(op) };

  function requireSaved(what: string): void {
    if (session().ws.dirty().length > 0) {
      throw new BackendError("git", `Save your changes before you ${what}.`);
    }
  }

  /** Reads the changed files into the core again. */
  async function reopen(): Promise<void> {
    await host.openHandle(session().handle);
  }

  async function network(): Promise<Network> {
    const companion = await host.companionCredentials();
    if (!companion) {
      throw new BackendError(
        "unsupported",
        `${NO_COMPANION_MESSAGE} Run it, open the pairing link it prints, then try again.`,
      );
    }
    return {
      http: companionHttp(companion),
      credentials: async (h) => {
        const s = await store();
        const [username, password] = await Promise.all([s.get(h, "username"), s.get(h, "password")]);
        return username !== null && password !== null ? { username, password } : null;
      },
    };
  }

  const requireIdentity = (): GitIdentity => {
    const id = identity.get();
    if (!id) throw new BackendError("git", "Set your name and email for commits first.");
    return id;
  };

  return {
    status: () => run(async () => (await ops()).status(repo())),

    diff: (path) =>
      run(async () => {
        const [o, core] = [await ops(), await host.deps.loadCore()];
        const [old, current] = await o.diffSides(repo(), path);
        return core.textDiff(path, old, current);
      }),

    commit: (message, paths) =>
      run(async () => {
        // The secret scanner first (wave 5): no commit while the files hold credentials.
        await refuseSecrets(session().handle, paths, await host.deps.loadCore());
        return (await ops()).commit(repo(), message, paths, requireIdentity());
      }),

    log: (limit) => run(async () => (await ops()).log(repo(), limit)),

    branches: () => run(async () => (await ops()).branches(repo())),

    checkout: (branch) =>
      run(async () => {
        requireSaved("switch branches");
        await (await ops()).checkout(repo(), branch);
        await reopen();
      }),

    createBranch: (name, checkout) => run(async () => (await ops()).createBranch(repo(), name, checkout)),

    push: () => run(async () => (await ops()).push(repo(), await network())),

    pull: () =>
      run(async () => {
        requireSaved("pull");
        const result = await (await ops()).pull(repo(), await network(), identity.get());
        if (result.outcome !== "up-to-date") await reopen();
        return result;
      }),

    conflict: (path) => run(async () => (await ops()).conflict(repo(), path)),

    resolve: (path, resolution) =>
      run(async () => {
        requireSaved("resolve a conflict");
        await (await ops()).resolve(repo(), path, resolution);
        await reopen();
      }),

    init: () => run(async () => (await ops()).init(repo())),

    clone: (url) =>
      run(async (): Promise<CollectionInfo | null> => {
        const trimmed = url.trim();
        if (looksLocal(trimmed)) throw new BackendError("unsupported", LOCAL_CLONE_UNSUPPORTED);
        if (!/^https?:\/\//i.test(trimmed)) {
          throw new BackendError("git", "Enter an HTTPS URL of the repository (SSH needs the desktop app).");
        }
        const net = await network();
        // The picker needs the click's user activation: ask before loading anything slow.
        const loadOps = ops();
        loadOps.catch(() => undefined);
        if (typeof window.showDirectoryPicker !== "function") {
          throw new BackendError("unsupported", "This browser cannot open local folders. Use a current Chrome or Edge.");
        }
        let parent: FileSystemDirectoryHandle;
        try {
          parent = await window.showDirectoryPicker({ id: "requesttailor-clone", mode: "readwrite" });
        } catch (e) {
          if (isAbort(e)) return null;
          throw e;
        }
        const o = await loadOps;
        const name = o.cloneFolderName(trimmed);
        const exists = await parent.getDirectoryHandle(name).then(
          () => true,
          () => false,
        );
        if (exists) throw new BackendError("git", `The folder "${name}" already exists in "${parent.name}".`);
        const target = await parent.getDirectoryHandle(name, { create: true });
        try {
          await o.clone(repoAt(target), net, trimmed);
        } catch (e) {
          await parent.removeEntry(name, { recursive: true }).catch(() => undefined);
          throw e;
        }
        return host.openHandle(target);
      }),

    identity: async () => identity.get(),

    setIdentity: async (id) => identity.set(validIdentity(id)),

    hasCredentials: async (h) => {
      const s = await store();
      return (await s.has(h, "username")) && (await s.has(h, "password"));
    },

    setCredentials: async (h, username, password) => {
      if (!h) throw new BackendError("git", "No host for the credentials.");
      const s = await store();
      await s.set(h, "username", username);
      await s.set(h, "password", password);
    },

    deleteCredentials: async (h) => {
      const s = await store();
      await s.delete(h, "username");
      await s.delete(h, "password");
    },

    // Wave 5, strand E: `rt_app::secret_scan` through rt-wasm (./scan.ts); `commit` refuses
    // with `secrets-found`.
    scan: (paths) => run(async () => scanPaths(session().handle, paths, await host.deps.loadCore())),

    // Wave 12 (interface): revisions, read without a checkout (./revisions.ts).
    revParse: (rev) => run(async () => (await revisions()).revParse(repo(), rev)),
    treeAt: (rev) => run(async () => (await revisions()).treeAt(repo(), rev)),
    show: (rev, path) => run(async () => (await revisions()).show(repo(), rev, path)),
    fileLog: (path, limit) => run(async () => (await revisions()).fileLog(repo(), path, limit)),
    mergeBase: (a, b) => run(async () => (await revisions()).mergeBase(repo(), a, b)),
    commits: (from, to, limit) => run(async () => (await revisions()).commits(repo(), from, to, limit)),

    // Wave 14 (interface): graph (strand F, ./graph.ts) and wizards (strand G, ./actions.ts).
    ...webGitGraph(wave14),
    ...webGitActions(wave14),
  };
}
