// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Where the `{{` completion of a CodeField gets its variables and creates new ones. The
// request editor and the environment manager provide one (`provideVarSource`); every
// CodeField below picks it up (`getVarSource`), so tables, body, auth and message fields
// complete without passing anything down. A field outside any provider has no completion.
//
// The scope is cached per collection, path and environment and refreshed when a field gets
// focus, so typing `{{` shows the list from memory at once.

import { getContext, setContext } from "svelte";

import type { Backend } from "../commands/backend";
import type { Environment, Variable, VariableScope } from "../commands/types";

export type CreateKind = "environment" | "collection" | "folder";

export interface CreateTarget {
  kind: CreateKind;
  /** "Environment", "Collection", "Folder". */
  label: string;
  /** The environment's or folder's name, the collection's name. */
  detail: string | null;
  /** Only environments keep secret values (in the keychain or the browser's secret store). */
  secretAllowed: boolean;
}

export interface NewVariable {
  name: string;
  value: string;
  secret: boolean;
}

export interface VarSource {
  /** The last scope loaded for the current collection, path and environment, or `null`. */
  cached(): VariableScope | null;
  /** Loads the scope (concurrent calls share one load). */
  load(): Promise<VariableScope>;
  /** Where "Create variable" can put a new one, most specific last. */
  targets(): CreateTarget[];
  /** Creates (or replaces) the variable, unsaved like any edit; secret values only through
   *  the secret store. Reloads the scope. */
  create(kind: CreateKind, variable: NewVariable): Promise<void>;
}

const KEY = Symbol("requesttailor-var-source");

export function provideVarSource(source: VarSource): VarSource {
  return setContext(KEY, source);
}

export function getVarSource(): VarSource | null {
  return getContext<VarSource | undefined>(KEY) ?? null;
}

/** The parts of the app state a source needs (`AppState` has them). */
export interface VarSourceApp {
  readonly backend: Backend;
  readonly collection: { id: string; name: string } | null;
  readonly environment: string | null;
  reloadEnvironments(): Promise<void>;
  dirty: string[];
}

interface Options {
  /** Request path, folder path or `""` (collection). */
  path: () => string;
  /** The environment whose layer applies (default: the active one). */
  environment?: () => string | null;
  /** Folder that "Create variable" offers (`null`: none). */
  folder?: () => string | null;
  /** Adds a variable to environment `name` instead of the default (the environment manager
   *  edits its own working copy). */
  addToEnvironment?: (name: string, variable: Variable) => Promise<void>;
  /** After a variable was created (the request editor refreshes its preview). */
  onchange?: () => void;
}

/** Folder of a request path: `users/admin/list.yml` → `users/admin`, `x.yml` → `null`. */
export function folderOf(requestPath: string): string | null {
  const at = requestPath.lastIndexOf("/");
  return at > 0 ? requestPath.slice(0, at) : null;
}

/** Puts `variable` into `env` (replacing one of the same name). */
export function withEnvironmentVariable(env: Environment, variable: Variable): Environment {
  const variables = env.variables.some((v) => v.name === variable.name)
    ? env.variables.map((v) => (v.name === variable.name ? variable : v))
    : [...env.variables, variable];
  return { ...env, variables };
}

export function createVarSource(app: VarSourceApp, options: Options): VarSource {
  const environment = () => (options.environment ? options.environment() : app.environment);
  const key = () => `${app.collection?.id ?? ""}\u0000${options.path()}\u0000${environment() ?? ""}`;
  let cache: { key: string; scope: VariableScope } | null = null;
  let loading: { key: string; promise: Promise<VariableScope> } | null = null;

  const load = (): Promise<VariableScope> => {
    const k = key();
    if (loading?.key === k) return loading.promise;
    const promise = app.backend.variables
      .scope(options.path(), { environment: environment(), vars: [] })
      .then((scope) => {
        if (key() === k) cache = { key: k, scope };
        return scope;
      })
      .finally(() => {
        if (loading?.promise === promise) loading = null;
      });
    loading = { key: k, promise };
    return promise;
  };

  return {
    cached: () => (cache?.key === key() ? cache.scope : null),
    load,
    targets() {
      const out: CreateTarget[] = [];
      const env = environment();
      if (env !== null) out.push({ kind: "environment", label: "Environment", detail: env, secretAllowed: true });
      out.push({ kind: "collection", label: "Collection", detail: app.collection?.name ?? null, secretAllowed: false });
      const folder = options.folder?.() ?? null;
      if (folder) {
        out.push({ kind: "folder", label: "Folder", detail: folder.split("/").pop() ?? folder, secretAllowed: false });
      }
      return out;
    },
    async create(kind, { name, value, secret }) {
      const backend = app.backend;
      if (kind === "environment") {
        const env = environment();
        if (env === null) throw new Error("No environment is active.");
        const variable: Variable = { name, value: secret ? null : value, enabled: true, secret };
        if (secret) await backend.secrets.set(env, name, value);
        if (options.addToEnvironment) {
          await options.addToEnvironment(env, variable);
        } else {
          const current = (await backend.environments()).find((e) => e.name === env);
          if (!current) throw new Error(`No environment named "${env}".`);
          await backend.updateEnvironment(withEnvironmentVariable(current, variable));
          await app.reloadEnvironments();
        }
      } else {
        if (secret) throw new Error("Secret values live in an environment.");
        const folder = kind === "folder" ? (options.folder?.() ?? null) : "";
        if (folder === null) throw new Error("This request is not in a folder.");
        await backend.variables.define(folder, { name, value, enabled: true, secret: false });
        app.dirty = await backend.dirty();
      }
      cache = null;
      loading = null;
      await load();
      options.onchange?.();
    },
  };
}
