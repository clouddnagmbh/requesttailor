// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Plain helpers of the plugin UI: what permissions mean in words, whether an update asks
// for more, which plugin tabs belong where. Owner: wave 5, strand A.

import type { Manifest, Permissions, PluginInfo, TabContribution, TabPlace } from "../commands/types";

/** One line per permission, for the install confirmation. Empty: the plugin asks for none. */
export function describePermissions(p: Permissions): string[] {
  const out: string[] = [];
  for (const host of p.network) {
    if (host === "*") out.push("Network: any host");
    else if (host === "{request}") out.push("Network: the host of the request being sent");
    else if (host.startsWith("*.")) out.push(`Network: subdomains of ${host.slice(2)}`);
    else out.push(`Network: ${host}`);
  }
  if (p.storage) out.push("Storage: keep its own data on this device");
  if (p.variables) out.push("Variables: read non-secret variables of the request");
  return out;
}

/** What `next` asks for beyond `granted` (an update's new permissions), described. Like the
 *  host (`permissions::covers`), a network entry is granted only if it was granted as is. */
export function addedPermissions(granted: Permissions, next: Permissions): string[] {
  const had = new Set(granted.network.map((n) => n.toLowerCase()));
  return describePermissions({
    network: next.network.filter((n) => !had.has(n.toLowerCase())),
    storage: next.storage && !granted.storage,
    variables: next.variables && !granted.variables,
  });
}

/** What installing `manifest` means given what is installed: new, update, or reinstall. */
export type InstallPlan =
  | { kind: "new"; permissions: string[] }
  | { kind: "update"; from: string; to: string; added: string[] };

export function installPlan(manifest: Manifest, installed: PluginInfo[]): InstallPlan {
  const current = installed.find((p) => p.manifest.id === manifest.id);
  if (!current) return { kind: "new", permissions: describePermissions(manifest.permissions) };
  return {
    kind: "update",
    from: current.manifest.version,
    to: manifest.version,
    added: addedPermissions(current.manifest.permissions, manifest.permissions),
  };
}

/** Media type without parameters, lower case (`application/json`). */
export function mediaType(value: string | null | undefined): string {
  return (value ?? "").split(";")[0]!.trim().toLowerCase();
}

/** Does a tab for `patterns` apply to `contentType`? Empty list: always. Patterns: an exact
 *  type, `type/*`, `*\/*`; `application/json` also covers `+json` types. */
export function matchesContentType(patterns: string[], contentType: string | null): boolean {
  if (patterns.length === 0) return true;
  const actual = mediaType(contentType);
  return patterns.some((raw) => {
    const p = mediaType(raw);
    if (p === "*/*" || p === actual) return true;
    if (p.endsWith("/*")) return actual.startsWith(p.slice(0, -1));
    const [type, sub] = p.split("/");
    return sub !== undefined && !sub.includes("+") && actual.startsWith(`${type}/`) && actual.endsWith(`+${sub}`);
  });
}

export interface PluginTabRef {
  /** `plugin/tab`, unique. */
  key: string;
  plugin: string;
  pluginName: string;
  tab: TabContribution;
}

/** Tabs of enabled plugins for `place`, in installation order; response tabs filtered by
 *  the response's content type (`null` for request tabs: no filter). */
export function tabsFor(plugins: PluginInfo[], place: TabPlace, contentType: string | null = null): PluginTabRef[] {
  return plugins
    .filter((p) => p.enabled)
    .flatMap((p) =>
      p.manifest.contributes.tabs
        .filter((t) => t.place === place && (place === "request" || matchesContentType(t.content_types, contentType)))
        .map((tab) => ({ key: `${p.manifest.id}/${tab.id}`, plugin: p.manifest.id, pluginName: p.manifest.name, tab })),
    );
}

/** The `content-type` header of `headers`, or `null`. */
export function contentTypeOf(headers: [string, string][]): string | null {
  return headers.find(([n]) => n.toLowerCase() === "content-type")?.[1] ?? null;
}

/** Importers of enabled plugins, for the import dialog. */
export function pluginImporters(plugins: PluginInfo[]) {
  return plugins
    .filter((p) => p.enabled)
    .flatMap((p) =>
      p.manifest.contributes.importers.map((imp) => ({
        value: `${p.manifest.id}/${imp.id}`,
        plugin: p.manifest.id,
        importer: imp.id,
        label: `${imp.label} (plugin ${p.manifest.name})`,
      })),
    );
}
