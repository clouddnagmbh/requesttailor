// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Additions a plugin proposes in a `ViewReply` (wave 6, strand F): `create_requests` and
// `add_environments` (crates/rt-plugin/API.md, "Views"). The app shows the plan first and
// applies it only when the user confirms; everything becomes an unsaved change, saved like
// any other edit.
//
// - A request whose name exists in its folder (or earlier in the same plan) gets the suffix
//   " 2", " 3", ... Folders are matched by directory or display name and created if missing.
// - An environment of an existing name keeps its variables and gets only the missing ones;
//   if none are missing it is left out. Secret variables are created without a value (the
//   user sets it; it goes to the key store), whatever value the plugin sent.

import type { Backend } from "../commands/backend";
import type { Environment, NewRequest, PluginEnvironment, RequestSpec, TreeNode, Variable } from "../commands/types";

export interface PlannedRequest {
  /** As the plugin gave it (`""`: the collection root). */
  folder: string;
  /** The final name, suffixed if taken. */
  name: string;
  /** The name the plugin asked for differs from `name`. */
  renamed: boolean;
  spec: RequestSpec;
}

export interface PlannedEnvironment {
  name: string;
  /** No environment of that name exists yet. */
  created: boolean;
  /** The variables to add; secrets have `value: null`. */
  variables: Variable[];
}

export interface AdditionPlan {
  requests: PlannedRequest[];
  environments: PlannedEnvironment[];
}

/** `true` if a reply proposes anything at all. */
export function hasAdditions(reply: { create_requests?: NewRequest[]; add_environments?: PluginEnvironment[] }): boolean {
  return (reply.create_requests?.length ?? 0) > 0 || (reply.add_environments?.length ?? 0) > 0;
}

export function isEmptyPlan(plan: AdditionPlan): boolean {
  return plan.requests.length === 0 && plan.environments.length === 0;
}

const segments = (folder: string) => folder.split("/").filter((s) => s !== "");
const key = (folder: string) => segments(folder).join("/").toLowerCase();
const lastSegment = (path: string) => path.split("/").at(-1) ?? "";

/** The folder node for `folder` (segments by directory or display name), or `null`. */
export function findFolder(tree: TreeNode | null, folder: string): (TreeNode & { kind: "folder" }) | null {
  if (!tree || tree.kind !== "folder") return null;
  let node: TreeNode & { kind: "folder" } = tree;
  for (const seg of segments(folder)) {
    const want = seg.toLowerCase();
    const next: (TreeNode & { kind: "folder" }) | undefined = node.children.find(
      (c): c is TreeNode & { kind: "folder" } =>
        c.kind === "folder" && (lastSegment(c.path).toLowerCase() === want || c.name.toLowerCase() === want),
    );
    if (!next) return null;
    node = next;
  }
  return node;
}

/** The first of `name`, `name 2`, `name 3`, ... not in `taken` (case-insensitive). */
export function freeName(name: string, taken: Set<string>): string {
  const base = name.trim() || "Request";
  if (!taken.has(base.toLowerCase())) return base;
  for (let n = 2; ; n++) {
    const candidate = `${base} ${n}`;
    if (!taken.has(candidate.toLowerCase())) return candidate;
  }
}

/** What applying the reply would add, against the collection as it is now. */
export function planAdditions(
  reply: { create_requests?: NewRequest[]; add_environments?: PluginEnvironment[] },
  tree: TreeNode | null,
  environments: Environment[],
): AdditionPlan {
  const taken = new Map<string, Set<string>>();
  const namesIn = (folder: string) => {
    // One set per folder, however the plugin spelled it.
    const node = findFolder(tree, folder);
    const k = node ? `\0${node.path}` : key(folder);
    let names = taken.get(k);
    if (!names) {
      names = new Set((node?.children ?? []).filter((c) => c.kind === "request").map((c) => c.name.toLowerCase()));
      taken.set(k, names);
    }
    return names;
  };
  const requests = (reply.create_requests ?? []).map((r): PlannedRequest => {
    const names = namesIn(r.folder);
    const name = freeName(r.name, names);
    names.add(name.toLowerCase());
    return { folder: segments(r.folder).join("/"), name, renamed: name !== r.name, spec: { ...structuredClone(r.spec), name } };
  });

  const planned = new Map<string, PlannedEnvironment>();
  for (const e of reply.add_environments ?? []) {
    const name = e.name.trim();
    if (!name) continue;
    const existing = environments.find((x) => x.name === name);
    const p = planned.get(name) ?? { name, created: !existing, variables: [] };
    for (const v of e.variables) {
      if (!v.name) continue;
      if (existing?.variables.some((x) => x.name === v.name)) continue;
      if (p.variables.some((x) => x.name === v.name)) continue;
      p.variables.push({ name: v.name, value: v.secret ? null : v.value, enabled: true, secret: v.secret });
    }
    planned.set(name, p);
  }
  const envs = [...planned.values()].filter((e) => e.created || e.variables.length > 0);
  return { requests, environments: envs };
}

/** Applies a plan through the backend (unsaved). Returns the paths of the new requests. */
export async function applyAdditions(backend: Backend, plan: AdditionPlan): Promise<string[]> {
  const paths: string[] = [];
  for (const r of plan.requests) {
    const folder = r.folder ? await backend.createFolder(r.folder) : "";
    const path = await backend.createRequest(folder, r.name);
    await backend.updateRequest(path, { ...r.spec, name: r.name });
    paths.push(path);
  }
  if (plan.environments.length > 0) {
    const current = await backend.environments();
    for (const e of plan.environments) {
      const existing = current.find((x) => x.name === e.name);
      const env: Environment = existing
        ? { ...existing, variables: [...existing.variables, ...e.variables.filter((v) => !existing.variables.some((x) => x.name === v.name))] }
        : { name: e.name, variables: e.variables, client_certificates: [], external_secrets: null };
      await backend.updateEnvironment(env);
    }
  }
  return paths;
}
