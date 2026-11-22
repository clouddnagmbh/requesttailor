// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `HistoryApi` of the web app, persisted in IndexedDB. `record` is called by the backend after
// every send. Owner: wave 4, strand E.
//
// - Recording, filtering and pruning run in the core (`HistoryLog`, crates/rt-wasm/src/
//   history_js.rs): the same code as on the desktop. This file only stores what it returns.
// - Entries hold no bodies, no header values and the masked URL; the report handed to the
//   core is stripped of bodies and headers before it crosses the boundary.
// - Records are queued, so `list` and `clear` always see every send before them.
// - IndexedDB unavailable (private window, blocked site data): the history works in memory
//   for this page (./history/store.ts).

import type { HistoryApi, HistoryEntry, RequestSpec, SendReport, StoredResponse } from "@requesttailor/ui";

import { openHistoryStore, type HistoryStore } from "./history/store";
import type { CoreModule } from "./wasm";
import type { WebHost } from "./web-backend";

/** `HistoryLog` of rt-wasm (history_js.rs). */
interface CoreHistoryLog {
  record(
    collection: string,
    name: string,
    method: string,
    at: number,
    report: SendReport,
  ): { entry: HistoryEntry; dropped: number[] };
  list(collection: string, query: string, limit: number): HistoryEntry[];
  clear(collection: string): number[];
  free(): void;
}

type HistoryCore = CoreModule & { HistoryLog: new (entries: unknown[]) => CoreHistoryLog };

interface Ready {
  log: CoreHistoryLog;
  store: HistoryStore;
}

/** The report without anything the history does not keep (bodies, header values). */
function slim(report: SendReport): SendReport {
  return {
    ...report,
    request: report.request && { ...report.request, headers: [] },
    response: report.response && {
      ...report.response,
      headers: [],
      body: { encoding: "utf8", data: "" },
    },
    variables: [],
  };
}

export class WebHistory implements HistoryApi {
  private ready: Promise<Ready> | null = null;
  private queue: Promise<unknown> = Promise.resolve();

  constructor(
    private readonly host: WebHost,
    private readonly openStore: () => Promise<HistoryStore> = openHistoryStore,
  ) {}

  private load(): Promise<Ready> {
    this.ready ??= (async () => {
      const core = (await this.host.deps.loadCore()) as HistoryCore;
      const store = await this.openStore();
      return { log: new core.HistoryLog(await store.all()), store };
    })().catch((e: unknown) => {
      this.ready = null;
      throw e;
    });
    return this.ready;
  }

  private collectionId(): string | null {
    try {
      return this.host.session().id;
    } catch {
      return null;
    }
  }

  /** Never throws into the send. */
  record(collection: string, spec: RequestSpec, result: StoredResponse): void {
    const at = Date.now();
    const report = slim(result.report);
    this.queue = this.queue
      .then(async () => {
        const { log, store } = await this.load();
        const { entry, dropped } = log.record(collection, spec.name, spec.method, at, report);
        await store.put(entry);
        await store.delete(dropped);
      })
      .catch((e: unknown) => console.warn("RequestTailor: recording the send in the history failed:", e));
  }

  async list(query: string, limit: number): Promise<HistoryEntry[]> {
    const id = this.collectionId();
    if (!id) return [];
    await this.queue;
    return (await this.load()).log.list(id, query, Math.max(0, Math.floor(limit)));
  }

  async clear(): Promise<void> {
    const id = this.collectionId();
    if (!id) return;
    const run = this.queue.then(async () => {
      const { log, store } = await this.load();
      await store.delete(log.clear(id));
    });
    this.queue = run.catch(() => {});
    await run;
  }
}
