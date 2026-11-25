// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `JournalApi` of the web app (wave 19, strand C), over rt-wasm's `Workspace.journal*`
// (crates/rt-wasm/src/journal_js.rs). The journal itself runs in the core: it is recorded in
// memory on every send while "Track test data" is on (it starts off in each page load), and
// this file keeps what the core queues in IndexedDB (`requesttailor-journal`, one record per
// collection: the entries and the id path), after each call and on a timer. Nothing here is
// ever written into a collection file; a browser without IndexedDB keeps the journal for this
// page only.

import type {
  CompensationItem,
  EntryStates,
  JournalApi,
  JournalEntry,
  JournalPage,
  JournalQuery,
  JournalSettings,
  ManualRevert,
  PluginCompensationRule,
  PluginInfo,
  PreRead,
  RequestSpec,
  RevertOptions,
  RevertOutcome,
  RevertPlan,
  SendInput,
} from "@requesttailor/ui";

import type { RunContext } from "./runner";
import { toBackendError } from "./wasm";
import type { Session } from "./web-backend";

/** One change the core queued for storage. */
export type JournalChange =
  | { op: "append"; entry: JournalEntry }
  | { op: "rewrite"; entries: JournalEntry[] }
  | { op: "settings"; settings: JournalSettings };

/** The journal's part of rt-wasm's `Workspace`. */
export interface CoreJournalWorkspace {
  journalOpen(entries: JournalEntry[], settings: JournalSettings | null): void;
  journalSettings(): JournalSettings;
  journalSetSettings(settings: JournalSettings): void;
  journalEntries(query: JournalQuery): JournalPage;
  journalClear(): void;
  journalDrain(): JournalChange[];
  journalStates(ids: number[]): EntryStates[];
  journalPlanManual(revert: ManualRevert, input: RunContext["input"]): Promise<RevertPlan>;
  journalPlanCompensations(items: CompensationItem[], input: RunContext["input"]): Promise<RevertPlan>;
  journalExecute(
    plan: RevertPlan,
    options: RevertOptions,
    input: RunContext["input"],
    companion: RunContext["companion"],
    services: RunContext["services"],
  ): Promise<RevertOutcome>;
  journalPreRead(
    request: RequestSpec,
    input: RunContext["input"],
    companion: RunContext["companion"],
    services: RunContext["services"],
  ): Promise<PreRead>;
}

export interface JournalHost {
  session(): Session;
  /** What a send needs (secret values, companion, services), as for the runner. */
  sendContext(input: SendInput): Promise<RunContext>;
  /** The installed plugins (for the compensation rules they ship). */
  plugins(): Promise<PluginInfo[]>;
}

export const JOURNAL_DB = "requesttailor-journal";
export const JOURNAL_STORE = "collections";

interface Stored {
  id: string;
  entries: JournalEntry[];
  id_path?: string;
}

/** Where the entries are kept between page loads. Never throws. */
export interface JournalStore {
  load(id: string): Promise<Stored | null>;
  save(record: Stored): Promise<void>;
}

const memoryStore: JournalStore = {
  load: async () => null,
  save: async () => {},
};

function request<T>(r: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    r.onsuccess = () => resolve(r.result);
    r.onerror = () => reject(r.error);
  });
}

export async function openJournalStore(): Promise<JournalStore> {
  try {
    const open = indexedDB.open(JOURNAL_DB, 1);
    open.onupgradeneeded = () => open.result.createObjectStore(JOURNAL_STORE, { keyPath: "id" });
    const db = await request(open);
    let broken = false;
    return {
      async load(id) {
        try {
          return ((await request(db.transaction(JOURNAL_STORE, "readonly").objectStore(JOURNAL_STORE).get(id))) as Stored | undefined) ?? null;
        } catch {
          return null;
        }
      },
      async save(record) {
        if (broken) return;
        try {
          await request(db.transaction(JOURNAL_STORE, "readwrite").objectStore(JOURNAL_STORE).put(record));
        } catch (e) {
          broken = true;
          console.warn("RequestTailor: the journal is no longer saved in this browser:", e);
        }
      },
    };
  } catch (e) {
    console.warn("RequestTailor: the journal is kept in memory only:", e);
    return memoryStore;
  }
}

/** Seconds of quiet before what the core queued is written. */
const SAVE_DELAY_MS = 500;

interface Opened {
  mirror: JournalEntry[];
  idPath: string | undefined;
  timer: ReturnType<typeof setTimeout> | null;
}

export function webJournal(host: JournalHost, openStore: () => Promise<JournalStore> = openJournalStore): JournalApi {
  let store: Promise<JournalStore> | null = null;
  const opened = new Map<string, Opened>();
  const getStore = () => (store ??= openStore());

  const ws = () => host.session().ws as unknown as CoreJournalWorkspace;

  /** Applies the queued changes to the mirror and writes it. */
  async function persist(id: string, state: Opened): Promise<void> {
    state.timer = null;
    const changes = ws().journalDrain();
    for (const change of changes) {
      if (change.op === "append") state.mirror.push(change.entry);
      else if (change.op === "rewrite") state.mirror = change.entries;
      else state.idPath = change.settings.id_path;
    }
    if (changes.length === 0) return;
    await (await getStore()).save({ id, entries: state.mirror, id_path: state.idPath });
  }

  function schedule(id: string, state: Opened) {
    if (state.timer !== null) return;
    state.timer = setTimeout(() => void persist(id, state).catch(() => {}), SAVE_DELAY_MS);
  }

  async function ensure(): Promise<{ id: string; state: Opened; core: CoreJournalWorkspace }> {
    const session = host.session();
    const id = session.id;
    let state = opened.get(id);
    const core = ws();
    if (!state) {
      const saved = await (await getStore()).load(id);
      state = { mirror: saved?.entries ?? [], idPath: saved?.id_path, timer: null };
      // "Track test data" is never remembered: it starts off.
      core.journalOpen(state.mirror, saved?.id_path ? { enabled: false, id_path: saved.id_path } : null);
      opened.set(id, state);
      if (typeof window !== "undefined") {
        window.addEventListener("pagehide", () => void persist(id, state!).catch(() => {}));
      }
    }
    return { id, state, core };
  }

  async function guard<T>(f: (j: { id: string; state: Opened; core: CoreJournalWorkspace }) => Promise<T> | T): Promise<T> {
    try {
      const j = await ensure();
      try {
        return await f(j);
      } finally {
        schedule(j.id, j.state);
      }
    } catch (e) {
      throw toBackendError(e);
    }
  }

  /** The send input of an environment, with its secret values. */
  const inputOf = (environment: string | null) => host.sendContext({ environment, vars: [] });

  return {
    settings: () => guard(({ core }) => core.journalSettings()),
    setSettings: (settings) => guard(({ core }) => core.journalSetSettings(settings)),
    entries: (query) => guard(({ core }) => core.journalEntries(query)),
    clear: () => guard(({ core }) => core.journalClear()),
    planManual: (revert) =>
      guard(async ({ core }) => {
        const entry = core.journalEntries({ limit: 10_000 }).entries.find((e) => e.id === revert.entry_id);
        const context = await inputOf(entry?.environment ?? null);
        return core.journalPlanManual(revert, context.input);
      }),
    planCompensations: (items) =>
      guard(async ({ core }) => {
        const first = items[0] && core.journalEntries({ limit: 10_000 }).entries.find((e) => e.id === items[0]!.entry_id);
        const context = await inputOf(first?.environment ?? null);
        return core.journalPlanCompensations(items, context.input);
      }),
    execute: (plan, options) =>
      guard(async ({ core }) => {
        const context = await inputOf(plan.environment);
        return core.journalExecute(plan, options, context.input, context.companion, context.services);
      }),
    pluginRules: () =>
      guard(async () =>
        (await host.plugins())
          .filter((p) => p.enabled)
          .flatMap((p): PluginCompensationRule[] =>
            (p.manifest.contributes.compensations ?? []).map((rule) => ({ plugin: p.manifest.id, rule })),
          ),
      ),
    preRead: (spec, environment) =>
      guard(async ({ core }) => {
        const context = await inputOf(environment);
        return core.journalPreRead(spec, context.input, context.companion, context.services);
      }),
    states: (entryIds) => guard(({ core }) => core.journalStates(entryIds)),
  };
}
