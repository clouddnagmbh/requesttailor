// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Where the web app keeps its request history: IndexedDB database `requesttailor-history`,
// one record per `HistoryEntry`, keyed by `id`. Storage may be unavailable (private windows,
// blocked site data) or fail later (quota): then the history lives in memory for this page
// only. Nothing here ever throws.

import type { HistoryEntry } from "@requesttailor/ui";

export const HISTORY_DB = "requesttailor-history";
export const HISTORY_STORE = "entries";

export interface HistoryStore {
  /** Every stored entry, any order. */
  all(): Promise<unknown[]>;
  put(entry: HistoryEntry): Promise<void>;
  delete(ids: number[]): Promise<void>;
}

function request<T>(r: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    r.onsuccess = () => resolve(r.result);
    r.onerror = () => reject(r.error);
  });
}

function done(tx: IDBTransaction): Promise<void> {
  return new Promise((resolve, reject) => {
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
    tx.onabort = () => reject(tx.error);
  });
}

class IndexedDbStore implements HistoryStore {
  private broken = false;

  constructor(private readonly db: IDBDatabase) {}

  private async write(f: (s: IDBObjectStore) => void): Promise<void> {
    if (this.broken) return;
    try {
      const tx = this.db.transaction(HISTORY_STORE, "readwrite");
      f(tx.objectStore(HISTORY_STORE));
      await done(tx);
    } catch (e) {
      // Quota, a closed database: keep going in memory, say so once.
      this.broken = true;
      console.warn("RequestTailor: the history is no longer saved in this browser:", e);
    }
  }

  async all(): Promise<unknown[]> {
    try {
      return await request(this.db.transaction(HISTORY_STORE, "readonly").objectStore(HISTORY_STORE).getAll());
    } catch {
      return [];
    }
  }

  put(entry: HistoryEntry): Promise<void> {
    return this.write((s) => void s.put(entry));
  }

  delete(ids: number[]): Promise<void> {
    if (ids.length === 0) return Promise.resolve();
    return this.write((s) => {
      for (const id of ids) s.delete(id);
    });
  }
}

/** Keeps nothing: the in-memory history (rt-wasm) is all there is. */
export const memoryStore: HistoryStore = {
  all: async () => [],
  put: async () => {},
  delete: async () => {},
};

/** The IndexedDB store, or the memory store if IndexedDB is unavailable. */
export async function openHistoryStore(): Promise<HistoryStore> {
  try {
    const open = indexedDB.open(HISTORY_DB, 1);
    open.onupgradeneeded = () => open.result.createObjectStore(HISTORY_STORE, { keyPath: "id" });
    return new IndexedDbStore(await request(open));
  } catch (e) {
    console.warn("RequestTailor: the history is kept in memory only:", e);
    return memoryStore;
  }
}
