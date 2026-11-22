// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `RescueApi` of the web app (wave 12, strand G): the history scan and the rewrite of
// ./git/rewrite.ts on the collection's folder (isomorphic-git), with what is found and how it
// is replaced from the core (`crates/rt-wasm/src/rescue_js.rs`). The value goes from the old
// blob into the encrypted browser secret store of the open collection (the plan's environment
// and variable) and never into a result or an error. Refuses with unsaved changes; reopens
// the collection after a rewrite. No push: the UI shows the force-push command.

import { BackendError, type RescueApi } from "@requesttailor/ui";

import { directoryFs } from "./git/fs";
import type { Repo } from "./git/repo";
import type { RescueCore } from "./git/rewrite";
import { toBackendError } from "./wasm";
import type { Wave12Host } from "./wave12";

type RewriteModule = typeof import("./git/rewrite");

let loading: Promise<RewriteModule> | null = null;
function ops(): Promise<RewriteModule> {
  loading ??= import("./git/rewrite").catch((e: unknown) => {
    loading = null;
    throw e;
  });
  return loading;
}

async function guard<T>(f: () => Promise<T>): Promise<T> {
  try {
    return await f();
  } catch (e) {
    throw toBackendError(e);
  }
}

export function webRescue(w: Wave12Host): RescueApi {
  const { host } = w;

  async function open(): Promise<{ repo: Repo; core: RescueCore }> {
    const s = host.session();
    const repo: Repo = { fs: directoryFs(s.handle), dir: "/", cache: {} };
    const status = await w.git.status();
    if (!status.repo) throw new BackendError("git", "The collection is not in a Git repository.");
    const core = (await host.deps.loadCore()) as unknown as RescueCore;
    return { repo, core };
  }

  return {
    scanHistory: (onProgress) =>
      guard(async () => {
        const { repo, core } = await open();
        return (await ops()).scanHistory(repo, core, onProgress);
      }),

    rewrite: (plan) =>
      guard(async () => {
        const s = host.session();
        if (s.ws.dirty().length > 0) {
          throw new BackendError("git", "Save or discard your changes before you rewrite the history.");
        }
        const { repo, core } = await open();
        const secrets = await host.deps.openSecrets(s.id);
        const now = Math.floor(Date.now() / 1000);
        const result = await (await ops()).rewriteHistory(repo, core, plan, now, (value) =>
          secrets.set(plan.environment, plan.variable, value),
        );
        await host.openHandle(s.handle);
        return result;
      }),
  };
}
