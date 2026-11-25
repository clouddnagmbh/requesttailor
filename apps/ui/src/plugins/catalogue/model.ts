// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Plain helpers of the plugin catalogue (wave 18, strand I): what is listed, what is said about
// a source and about the rights, and how a failure is explained. No state, no backend.

import type {
  CatalogueEntry,
  CatalogueSource,
  CatalogueUpdate,
  InstalledSource,
  InstallPlan,
  Permissions,
  SourceIndex,
} from "../../commands/types";
import { describePermissions } from "../permissions";

/** What a source is called: the index's own name, else the address the user typed. */
export function sourceLabel(s: CatalogueSource): string {
  return s.name?.trim() ? s.name : s.url;
}

/** "index at 3fa1c9e" for a Git source read at a commit, else `null`. */
export function commitNote(s: CatalogueSource): string | null {
  return s.commit ? `index at ${s.commit.slice(0, 7)}` : null;
}

/** One entry of a refreshed source, with what is known about it locally. */
export interface Row {
  source: CatalogueSource;
  entry: CatalogueEntry;
  /** The installed version of this plugin, if one is installed. */
  installed: string | null;
  /** The version in the index is newer than the one installed from this source. */
  update: boolean;
}

/** Entries of `indexes` whose id, name or description contain every word of `query`. */
export function rows(indexes: SourceIndex[], installed: InstalledSource[], updates: CatalogueUpdate[], query: string): Row[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  const out: Row[] = [];
  for (const { source, index } of indexes) {
    for (const entry of index.plugins) {
      const hay = `${entry.id} ${entry.name} ${entry.description ?? ""}`.toLowerCase();
      if (!words.every((w) => hay.includes(w))) continue;
      const have = installed.find((i) => i.plugin === entry.id);
      out.push({
        source,
        entry,
        installed: have?.version ?? null,
        update: updates.some((u) => u.plugin === entry.id && u.source === source.url),
      });
    }
  }
  return out;
}

/** The rights the index says an entry has (shown only as "the index says"). */
export function claimedRights(entry: CatalogueEntry): string[] {
  return describePermissions(entry.permissions ?? { network: [], storage: false, variables: false });
}

/** The rights of a plan: they come from the module's own manifest. */
export function planRights(plan: InstallPlan): string[] {
  return describePermissions(plan.permissions);
}

export function noRights(p: Permissions): boolean {
  return describePermissions(p).length === 0;
}

/** A short form of a checksum for a line of text ("3fa1c9e0…b21d"). */
export function shortSum(sha: string): string {
  return sha.length > 16 ? `${sha.slice(0, 8)}…${sha.slice(-4)}` : sha;
}

/** A failure that says the browser was not allowed to read the address (CORS and the like). */
export function looksBlocked(message: string): boolean {
  return /cors|failed to fetch|networkerror|load failed|blocked/i.test(message) && !/\bHTTP \d{3}\b|status \d{3}/i.test(message);
}

/** What to tell the user when a source or a download failed. */
export function explain(message: string): string {
  return looksBlocked(message)
    ? `${message} If this is the web app, pair the companion (rt agent) or install the plugin file from "Installed".`
    : message;
}
