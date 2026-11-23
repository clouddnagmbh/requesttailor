// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `VariablesApi` of the mock backend (wave 14, variable completion), with the same rules as
// `rt_app::variables`: layers collection < environment < folders (outer to inner) < request <
// runtime, names without values listed too, secret values never returned. The mock keeps
// collection and folder variables here (sample: `apiVersion` in the collection, `userId` in
// folder `users`); its send resolves them through `lookup`. `define` stages them until `save`.

import { BackendError, type SendInput, type VariablesApi } from "./backend";
import type { Environment, FileChange, RequestSpec, ScopeVar, VariableScope, VarSource, Variable, VarChange } from "./types";

export interface MockVariablesHost {
  requests(): Map<string, RequestSpec>;
  environments(): Environment[];
  sessionVars(environment: string): Promise<VarChange[]>;
  /** Secret values of `environment` (to mask copies in runtime values). */
  secretValues(environment: string | null): string[];
}

export interface MockVariables extends VariablesApi {
  /** A collection or folder variable for the send (`folder` layers above the environment,
   *  the collection below it): value and source, or `undefined`. */
  lookup(path: string, name: string, layer: "folders" | "collection"): { value: string; source: VarSource } | undefined;
  dirty(): string[];
  save(): FileChange[];
}

const plain = (name: string, value: string): Variable => ({ name, value, enabled: true, secret: false });

/** Folders of a request path, outermost first: `a/b/r.yml` → `a`, `a/b`. */
export function folderChain(path: string): string[] {
  const parts = path.split("/");
  const isRequest = path.endsWith(".yml");
  const dirs = isRequest ? parts.slice(0, -1) : parts.filter((p) => p !== "");
  return dirs.map((_, i) => dirs.slice(0, i + 1).join("/"));
}

const fileOf = (folder: string) => (folder === "" ? "opencollection.yml" : `${folder}/folder.yml`);

export function mockVariables(host: MockVariablesHost): MockVariables {
  const vars = new Map<string, Variable[]>([
    ["", [plain("apiVersion", "v2")]],
    ["users", [plain("userId", "42")]],
  ]);
  const saved = new Map([...vars].map(([k, v]) => [k, structuredClone(v)]));
  const folders = () => {
    const set = new Set<string>();
    for (const p of host.requests().keys()) for (const f of folderChain(p)) set.add(f);
    return set;
  };
  const exists = (path: string) => path === "" || host.requests().has(path) || folders().has(path);

  return {
    async scope(path: string, input: SendInput): Promise<VariableScope> {
      if (!exists(path)) throw new BackendError("request-not-found", path);
      const masks = host.secretValues(input.environment).filter((v) => v !== "");
      const mask = (v: string) => masks.reduce((acc, s) => acc.split(s).join("***"), v);
      const out: ScopeVar[] = [];
      const push = (source: VarSource, list: Variable[]) => {
        for (const v of list.filter((x) => x.enabled)) {
          out.push({
            name: v.name,
            value: v.secret || v.value === null ? null : mask(v.value),
            source,
            secret: v.secret,
            external: null,
            overridden: false,
          });
        }
      };
      push({ layer: "collection" }, vars.get("") ?? []);
      const env = host.environments().find((e) => e.name === input.environment);
      if (env) {
        for (const s of env.external_secrets?.variables.filter((x) => x.enabled) ?? []) {
          out.push({
            name: s.name,
            value: null,
            source: { layer: "environment", name: env.name },
            secret: true,
            external: env.external_secrets!.provider,
            overridden: false,
          });
        }
        push({ layer: "environment", name: env.name }, env.variables);
      }
      for (const f of path.endsWith(".yml") || path === "" ? folderChain(path) : folderChain(`${path}/x.yml`)) {
        push({ layer: "folder", path: f }, vars.get(f) ?? []);
      }
      const spec = host.requests().get(path);
      if (spec) push({ layer: "request" }, spec.vars);
      const session = (await host.sessionVars(input.environment ?? "")).flatMap((c): [string, string][] =>
        c.value === null ? [] : [[c.name, c.value]],
      );
      for (const [name, value] of [...session, ...input.vars]) {
        const secret = out.some((v) => v.secret && v.name === name);
        out.push({ name, value: secret ? null : mask(value), source: { layer: "runtime" }, secret, external: null, overridden: false });
      }
      const seen = new Set<string>();
      for (let i = out.length - 1; i >= 0; i--) {
        out[i].overridden = seen.has(out[i].name);
        seen.add(out[i].name);
      }
      const used = new Set<string>();
      const scan = (text: string) => {
        for (const m of text.matchAll(/\{\{([^}]+)\}\}/g)) {
          const name = m[1];
          if (name.trim() === name && !name.includes("{") && !name.startsWith("$")) used.add(name);
        }
      };
      for (const s of host.requests().values()) scan(JSON.stringify({ ...s, scripts: [] }));
      for (const e of host.environments()) for (const v of e.variables) if (v.value) scan(v.value);
      return { variables: out, undefined: [...used].filter((n) => !seen.has(n)).sort().slice(0, 100) };
    },

    async define(folder: string, variable: Variable): Promise<void> {
      if (variable.secret) throw new BackendError("unsupported", "secret variables belong in an environment");
      const name = variable.name;
      if (name === "" || name.trim() !== name || /[{}]/.test(name)) {
        throw new BackendError("format", `not a variable name: ${JSON.stringify(name)}`);
      }
      if (folder !== "" && !folders().has(folder)) throw new BackendError("folder-not-found", folder);
      const list = [...(vars.get(folder) ?? [])];
      const at = list.findIndex((v) => v.name === name);
      if (at >= 0) list[at] = { ...variable };
      else list.push({ ...variable });
      vars.set(folder, list);
    },

    lookup(path, name, layer) {
      if (layer === "collection") {
        const v = (vars.get("") ?? []).find((x) => x.enabled && x.name === name && x.value !== null);
        return v ? { value: v.value!, source: { layer: "collection" } } : undefined;
      }
      for (const f of folderChain(path).reverse()) {
        const v = (vars.get(f) ?? []).find((x) => x.enabled && x.name === name && x.value !== null);
        if (v) return { value: v.value!, source: { layer: "folder", path: f } };
      }
      return undefined;
    },

    dirty() {
      return [...vars.keys()]
        .filter((k) => JSON.stringify(vars.get(k)) !== JSON.stringify(saved.get(k) ?? []))
        .map(fileOf)
        .sort();
    },

    save() {
      const changes: FileChange[] = [];
      for (const [k, list] of vars) {
        if (JSON.stringify(list) === JSON.stringify(saved.get(k) ?? [])) continue;
        saved.set(k, structuredClone(list));
        changes.push({ path: fileOf(k), text: `# mock rendering\n${JSON.stringify({ variables: list }, null, 2)}\n`, deleted: false });
      }
      return changes;
    },
  };
}
