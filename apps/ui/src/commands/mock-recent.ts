// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Recent collections in the mock backend, for the start flow's tests. Owner: wave 4, strand C.
// Kept in localStorage (if available), so a reload of `?backend=mock` behaves like a restart
// of the app: the last collection is reopened. A fresh browser profile starts empty.

import type { CollectionInfo, RecentCollection } from "./backend";

const KEY = "rt.mock.recent";
const MAX = 10;

function isRecent(v: unknown): v is RecentCollection {
  const r = v as RecentCollection;
  return (
    typeof v === "object" &&
    v !== null &&
    typeof r.id === "string" &&
    typeof r.name === "string" &&
    typeof r.location === "string" &&
    typeof r.openedAt === "number"
  );
}

export class MockRecent {
  /** Used when localStorage is missing or throws (unit tests, blocked site data). */
  private memory: RecentCollection[] = [];

  /** Called by `MockBackend.openCollection` with what it opened. */
  opened(info: CollectionInfo): void {
    const entry: RecentCollection = { id: info.id, name: info.name, location: info.location, openedAt: Date.now() };
    this.write([entry, ...this.list().filter((r) => r.id !== info.id)].slice(0, MAX));
  }

  list(): RecentCollection[] {
    try {
      const raw = globalThis.localStorage?.getItem(KEY);
      if (raw == null) return [...this.memory];
      const value: unknown = JSON.parse(raw);
      return Array.isArray(value) ? value.filter(isRecent) : [];
    } catch {
      return [...this.memory];
    }
  }

  private write(list: RecentCollection[]): void {
    this.memory = list;
    try {
      globalThis.localStorage?.setItem(KEY, JSON.stringify(list));
    } catch {
      // In memory only.
    }
  }
}
