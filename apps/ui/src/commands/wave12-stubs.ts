// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 12 interface: placeholder APIs that reject with `unsupported` until a strand replaces
// them in its own file (mock-*.ts here, the shells' own files in apps/web and apps/desktop).
// Exported from the package so the shells can use them until then.

import type { CompareApi, ContractApi, DiffApi, MergeApi, RescueApi, ReviewApi, TimelineApi } from "./backend";
import { BackendError } from "./backend";

function notYet(feature: string, strand: string): () => Promise<never> {
  return () => Promise.reject(new BackendError("unsupported", `${feature} arrives in wave 12 (strand ${strand}).`));
}

export function stubDiffs(): DiffApi {
  const no = notYet("The semantic diff", "A");
  return { working: no, commit: no, between: no };
}

export function stubMerges(): MergeApi {
  return {
    conflicts: async () => ({ merging: false, conflicts: [], text_conflicts: [] }),
    resolve: notYet("The semantic merge", "B"),
  };
}

export function stubCompare(): CompareApi {
  const no = notYet("Branch compare and bisect", "C");
  return { run: no, compare: no, bisect: no, cancel: async () => {} };
}

export function stubTimeline(): TimelineApi {
  const no = notYet("The request history", "D");
  return { history: no, blame: no, version: no, results: async () => [] };
}

export function stubReview(): ReviewApi {
  const no = notYet("Pull-request review", "E");
  return {
    repo: async () => null,
    hosts: async () => [],
    setHosts: no,
    hasToken: async () => false,
    setToken: no,
    deleteToken: no,
    startLogin: no,
    finishLogin: no,
    list: no,
    open: no,
    comments: no,
    comment: no,
    submit: no,
    ci: no,
  };
}

export function stubContract(): ContractApi {
  const no = notYet("The contract check", "F");
  return { spec: async () => null, check: no, changes: no, changelog: no };
}

export function stubRescue(): RescueApi {
  const no = notYet("Secret rescue", "G");
  return { scanHistory: no, rewrite: no };
}
