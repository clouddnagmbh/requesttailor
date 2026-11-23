// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// What the revision picker offers (wave 14, strand H): branches, remote branches, tags and
// recent commits, each on a coloured lane like in the commit graph (`--rt-lane-1…8`; a local
// branch and its remote share one), plus the one-click targets "Compare with main" and
// "Compare with remote". Works from `GitApi.refs` (with ahead/behind) and falls back to
// `GitApi.branches` where a shell has no refs yet.

import type { GitBranch, GitCommit, GitRef } from "../commands/types";

export type RevisionKind = "branch" | "remote" | "tag" | "commit";

export interface RevisionOption {
  /** What goes into the comparison (`main`, `origin/main`, `v1`, a commit id). */
  rev: string;
  label: string;
  kind: RevisionKind;
  /** 1…8 for `--rt-lane-<n>`; 0 for tags (no lane). */
  lane: number;
  /** Second line: a commit's summary, `2 ahead · 1 behind`, a tag's message. */
  detail?: string;
  current?: boolean;
}

export const LANES = 8;

/** `origin/feature/x` → `feature/x`; a local name stays. */
export function baseName(name: string, remote: boolean): string {
  if (!remote) return name;
  const slash = name.indexOf("/");
  return slash < 0 ? name : name.slice(slash + 1);
}

const MAIN_NAMES = ["main", "master", "trunk", "develop"];

/** Lanes by branch: the current branch first, then the main branch, then the rest by name. */
function lanes(names: { name: string; remote: boolean; current: boolean }[]): Map<string, number> {
  const bases = [...new Set(names.map((n) => baseName(n.name, n.remote)))];
  const current = names.find((n) => n.current && !n.remote)?.name;
  const main = MAIN_NAMES.find((m) => bases.includes(m));
  const order = [...(current ? [current] : []), ...(main && main !== current ? [main] : []), ...bases.filter((b) => b !== current && b !== main).sort()];
  return new Map(order.map((b, i) => [b, (i % LANES) + 1]));
}

function aheadBehind(r: GitRef): string | undefined {
  const parts = [r.ahead > 0 ? `${r.ahead} ahead` : "", r.behind > 0 ? `${r.behind} behind` : ""].filter(Boolean);
  if (r.upstream && parts.length === 0) return `in sync with ${r.upstream}`;
  return parts.length > 0 ? `${parts.join(" · ")}${r.upstream ? ` ${r.upstream}` : ""}` : undefined;
}

const commitOptions = (commits: GitCommit[], lane: number): RevisionOption[] =>
  commits.map((c) => ({ rev: c.oid, label: c.oid.slice(0, 7), kind: "commit", lane, detail: c.summary }));

/** The picker's options from `GitApi.refs` and recent commits of `HEAD`. */
export function optionsFromRefs(refs: GitRef[], commits: GitCommit[]): RevisionOption[] {
  const branches = refs.filter((r) => r.kind === "local" || r.kind === "remote");
  const lane = lanes(branches.map((r) => ({ name: r.name, remote: r.kind === "remote", current: r.current })));
  const sorted = [...branches].sort((a, b) => Number(b.current) - Number(a.current) || a.kind.localeCompare(b.kind) || a.name.localeCompare(b.name));
  const out: RevisionOption[] = sorted
    .filter((r) => !r.name.endsWith("/HEAD"))
    .map((r) => ({
      rev: r.name,
      label: r.name,
      kind: r.kind === "local" ? "branch" : "remote",
      lane: lane.get(baseName(r.name, r.kind === "remote")) ?? 1,
      ...(r.kind === "local" && aheadBehind(r) ? { detail: aheadBehind(r) } : {}),
      ...(r.current ? { current: true } : {}),
    }));
  for (const t of refs.filter((r) => r.kind === "tag").sort((a, b) => b.name.localeCompare(a.name, undefined, { numeric: true }))) {
    out.push({ rev: t.name, label: t.name, kind: "tag", lane: 0, ...(t.message ? { detail: t.message } : {}) });
  }
  const current = refs.find((r) => r.kind === "local" && r.current);
  out.push(...commitOptions(commits, current ? (lane.get(current.name) ?? 1) : 1));
  return out;
}

/** The same from `GitApi.branches` (no tags, no ahead/behind). */
export function optionsFromBranches(branches: GitBranch[], commits: GitCommit[]): RevisionOption[] {
  const lane = lanes(branches);
  const sorted = [...branches].sort((a, b) => Number(b.current) - Number(a.current) || Number(a.remote) - Number(b.remote) || a.name.localeCompare(b.name));
  const out: RevisionOption[] = sorted.map((b) => ({
    rev: b.name,
    label: b.name,
    kind: b.remote ? "remote" : "branch",
    lane: lane.get(baseName(b.name, b.remote)) ?? 1,
    ...(b.current ? { current: true } : {}),
  }));
  const current = branches.find((b) => b.current && !b.remote);
  out.push(...commitOptions(commits, current ? (lane.get(current.name) ?? 1) : 1));
  return out;
}

export interface QuickTargets {
  /** The current branch (`HEAD` when detached or unknown). */
  current: string;
  /** The main branch, unless it is the current one. */
  main?: string;
  /** The current branch's remote counterpart. */
  remote?: string;
}

/** The one-click comparisons: the main branch and the remote of the current branch, each
 *  against the current branch. */
export function quickTargets(options: RevisionOption[], upstream?: string): QuickTargets {
  const current = options.find((o) => o.current && o.kind === "branch")?.rev;
  const locals = options.filter((o) => o.kind === "branch").map((o) => o.rev);
  const remotes = options.filter((o) => o.kind === "remote").map((o) => o.rev);
  const main = MAIN_NAMES.find((m) => locals.includes(m)) ?? remotes.find((r) => MAIN_NAMES.includes(baseName(r, true)));
  const remote = upstream ?? (current ? remotes.find((r) => baseName(r, true) === current) : undefined);
  return {
    current: current ?? "HEAD",
    ...(main && main !== current ? { main } : {}),
    ...(remote ? { remote } : {}),
  };
}

/** Options whose label, rev or detail contain every word of `query` (any case). */
export function filterOptions(options: RevisionOption[], query: string): RevisionOption[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return options;
  return options.filter((o) => {
    const hay = `${o.label} ${o.rev} ${o.detail ?? ""}`.toLowerCase();
    return words.every((w) => hay.includes(w));
  });
}

export const KIND_LABEL: Record<RevisionKind, string> = {
  branch: "Branches",
  remote: "Remote branches",
  tag: "Tags",
  commit: "Recent commits",
};

/** How a revision reads in the picker's button: branches and tags by name, commits short. */
export function revisionLabel(rev: string): string {
  return /^[0-9a-f]{12,40}$/.test(rev) ? rev.slice(0, 7) : rev;
}
