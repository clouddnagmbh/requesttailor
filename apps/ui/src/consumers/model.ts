// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The pure side of the consumers view (wave 19, strand F): sentences and tones for a report.
// Nothing here decides what breaks or whether a request writes: the core answers
// (`rt_app::consumers`), and the run itself leaves out what must not run.

import type { ConsumerReport, ConsumerResult } from "../commands/types";

export type ResultTone = "ok" | "danger" | "warn";

export function isBroken(result: ConsumerResult): boolean {
  return result.failures.length > 0 || !!result.error;
}

export function tone(result: ConsumerResult): ResultTone {
  if (result.error) return "warn";
  return result.failures.length > 0 ? "danger" : "ok";
}

export const shortCommit = (commit: string): string => commit.slice(0, 7);

/** The line over one consumer. */
export function headline(result: ConsumerResult): string {
  if (result.error) return `${result.name} could not be verified`;
  if (result.failures.length > 0) {
    const n = result.failures.length;
    return `${result.name} breaks (${n} ${n === 1 ? "failure" : "failures"})`;
  }
  return `${result.name} is fine`;
}

/** "3 requests, 1 left out, commit 1a2b3c4". */
export function details(result: ConsumerResult): string {
  const parts = [`${result.requests} ${result.requests === 1 ? "request" : "requests"}`];
  if (result.skipped) parts.push(`${result.skipped} left out`);
  if (result.commit) parts.push(`commit ${shortCommit(result.commit)}`);
  return parts.join(", ");
}

export function summary(report: ConsumerReport): string {
  const broken = report.consumers.filter(isBroken).length;
  const n = report.consumers.length;
  return `${broken} of ${n} ${n === 1 ? "consumer" : "consumers"} broken`;
}

/** Why a writing request of a consumer was left out, for the hint under the form. */
export const WRITES_HINT =
  "Writing requests of consumers run only with “Allow writes” and never against an environment that looks like production.";

/** Where the consumer's collection comes from, as a short phrase. */
export function sourceLine(source: string, ref?: string): string {
  return ref ? `${source} @ ${ref}` : source;
}
