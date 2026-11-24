// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Pure helpers of the environment comparison (wave 18, strand A): the selection of
// environments, which ones are production, and the rows of one reference-against-other pair
// for the list. The rows are the ones of the revision comparison (`compare/rows.ts`), built
// without a semantic diff, so the list, the filters and the n/p keys behave the same.

import type { AgentPolicy, EnvComparison, EnvPair, RunComparison } from "../commands/types";
import { buildRows, type CompareRow } from "../compare/rows";
import { isProduction } from "../environments/production";

/** What a comparison needs at least. */
export const MIN_ENVIRONMENTS = 2;

/** Adds `name` to the selection (at the end) or removes it; order is the order of choosing,
 *  the first is the reference. */
export function toggle(selected: string[], name: string): string[] {
  return selected.includes(name) ? selected.filter((n) => n !== name) : [...selected, name];
}

/** Makes `name` the reference (first), keeping the others in order. */
export function makeReference(selected: string[], name: string): string[] {
  return selected.includes(name) ? [name, ...selected.filter((n) => n !== name)] : selected;
}

export const canCompare = (selected: string[]): boolean => selected.length >= MIN_ENVIRONMENTS;

/** Starting selection: the active environment first, then the first other one. */
export function defaultSelection(names: string[], active: string | null): string[] {
  const first = active && names.includes(active) ? active : names[0];
  const second = names.find((n) => n !== first);
  return first && second ? [first, second] : [];
}

/** The selected environments that are production (the policy flag, else the name). */
export function productionNames(policy: AgentPolicy | null, selected: string[]): string[] {
  return selected.filter((n) => isProduction(policy, n));
}

const NO_COMMIT = { oid: "", summary: "", author: "", time: 0 };

/** One pair as a `RunComparison` (the revision fields stay empty), what `buildRows` takes. */
export function asRun(pair: EnvPair): RunComparison {
  return { from: NO_COMMIT, to: NO_COMMIT, requests: pair.requests, from_summary: pair.reference_summary, to_summary: pair.summary };
}

/** One row per request of the pair, in the order they ran. */
export function rowsOfPair(pair: EnvPair): CompareRow[] {
  return buildRows(null, asRun(pair));
}

export interface PairCounts {
  total: number;
  different: number;
  same: number;
  /** Differences in the status code. */
  status: number;
  /** Requests that failed to send in one environment only. */
  errors: number;
}

export function pairCounts(pair: EnvPair): PairCounts {
  const different = pair.requests.filter((r) => !r.same);
  return {
    total: pair.requests.length,
    different: different.length,
    same: pair.requests.length - different.length,
    status: different.filter((r) => r.status !== null).length,
    errors: different.filter((r) => r.error !== null).length,
  };
}

/** The line above the list: `dev (reference) against qa: 2 of 5 requests differ`. */
export function pairTitle(cmp: EnvComparison, pair: EnvPair): string {
  const c = pairCounts(pair);
  return `${cmp.reference} (reference) against ${pair.environment}: ${c.different} of ${c.total} ${c.total === 1 ? "request differs" : "requests differ"}`;
}

/** Why a request was not sent, for the list of skipped ones. */
export function skippedLine(s: { name: string; path: string; reason: string }): string {
  return `${s.name} (${s.path}): ${s.reason}`;
}
