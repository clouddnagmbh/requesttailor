// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `PagingApi` of the mock backend (wave 18, strand C), for UI tests (`?backend=mock`). Rules live
// in memory (request and collection scope, same precedence as the core: request, collection,
// plugin). A rule makes the request's list a simulated five pages of two items; "Next" gives
// the next page alone, "All pages" the merged items, within the page limit. `?paging=plugin`
// offers a plugin's rule ("acme") for requests without one of their own. Only reading requests
// follow, as in the core. This is a simplified mirror: it never parses the real response.

import { BackendError, type PagingApi, type SendInput } from "./backend";
import type {
  EffectivePaging,
  PagedResult,
  PagingRun,
  PagingSpec,
  PagingStop,
  RequestSpec,
  SendReport,
  StoredResponse,
} from "./types";

export const MOCK_PAGES = 5;
const PER_PAGE = 2;

export interface MockPagingHost {
  getRequest(path: string): Promise<RequestSpec>;
  send(path: string, input: SendInput): Promise<StoredResponse>;
  /** Keeps `body` as the body of a new response built on `report`. */
  keep(report: SendReport, body: string): StoredResponse;
}

function pluginRule(): EffectivePaging | null {
  const on = new URLSearchParams(globalThis.location?.search ?? "").get("paging") === "plugin";
  return on
    ? {
        spec: { rule: { kind: "json-url", next: "$.next" }, merge: "$.items" },
        origin: { kind: "plugin", plugin: "acme", id: "next-link" },
      }
    : null;
}

function items(page: number): { id: number }[] {
  return Array.from({ length: PER_PAGE }, (_, k) => ({ id: (page - 1) * PER_PAGE + k + 1 }));
}

export function mockPaging(host: MockPagingHost): PagingApi {
  const requestRules = new Map<string, PagingSpec>();
  let collectionRule: PagingSpec | null = null;
  /** The page a kept response shows. */
  const pageOf = new Map<number, number>();

  const effective = (path: string): EffectivePaging | null => {
    const own = requestRules.get(path);
    if (own) return { spec: own, origin: { kind: "request" } };
    if (collectionRule) return { spec: collectionRule, origin: { kind: "collection" } };
    return pluginRule();
  };

  return {
    rule: async (path) => effective(path),
    setRule: async (path, spec, scope) => {
      if (scope === "collection") collectionRule = spec;
      else if (spec) requestRules.set(path, spec);
      else requestRules.delete(path);
    },
    run: async (run: PagingRun): Promise<PagedResult> => {
      const found = effective(run.path);
      if (!found) throw new BackendError("shell", "This request has no paging rule.");
      const spec = await host.getRequest(run.path);
      if (!["GET", "HEAD", "OPTIONS"].includes(spec.method.toUpperCase())) {
        throw new BackendError("shell", "Only requests that read follow pages: this one writes.");
      }
      const base = await host.send(run.path, run.input);
      const from = run.mode === "next" && run.response_id != null ? (pageOf.get(run.response_id) ?? 1) : 0;
      const limit = Math.min(1000, Math.max(1, run.limits?.max_pages ?? 20));
      let pages: number;
      let body: Record<string, unknown>;
      let stopped: PagingStop = { kind: "done" };
      let more = false;
      let last: number;
      if (run.mode === "next") {
        last = Math.min(MOCK_PAGES, from + 1);
        pages = 1;
        more = last < MOCK_PAGES;
        body = { page: last, items: items(last) };
      } else {
        pages = Math.min(MOCK_PAGES, limit);
        last = pages;
        if (pages < MOCK_PAGES) {
          stopped = { kind: "max-pages" };
          more = true;
        }
        body = { total: MOCK_PAGES * PER_PAGE, page: 1, items: Array.from({ length: pages }, (_, k) => items(k + 1)).flat() };
      }
      const report: SendReport = structuredClone(base.report);
      report.notes = [
        ...(report.notes ?? []),
        {
          source: "paging",
          level: stopped.kind === "done" ? "info" : "warning",
          text: stopped.kind === "done" ? `${pages} pages` : `stopped after ${pages} pages: limit reached; more pages exist`,
        },
      ];
      const kept = host.keep(report, JSON.stringify(body, null, 2));
      if (kept.response_id !== null) pageOf.set(kept.response_id, run.mode === "next" ? last : MOCK_PAGES);
      if (base.response_id !== null) {
        /* the base response stays kept: the caller releases the one it replaces */
      }
      return { response: kept, pages, stopped, more };
    },
  };
}
