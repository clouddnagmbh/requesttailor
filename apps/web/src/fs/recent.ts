// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Remembers the last opened folder: its directory handle and the collection id we gave it
// (random, stable per folder on this browser profile; used as `SecretKey.collection`), and
// the list of recently opened collections (`../recent.ts`). Directory handles can only be
// stored in IndexedDB. Storage may be unavailable (private windows, blocked site data): then
// nothing is remembered and every open shows the picker.
//
// Keys in the store: `last` and `folder:<id>` (a `RememberedFolder`), `recent` (the list,
// `RecentEntry[]`, most recent first), `roots` (wave 14: the folders in the sidebar and the
// active collection, `RememberedRoots`).

const DB = "requesttailor-web";
const STORE = "folders";
const LAST = "last";
const RECENT = "recent";
const ROOTS = "roots";
/** Entries kept in the recent list. */
export const MAX_RECENT = 10;

export interface RememberedFolder {
  id: string;
  handle: FileSystemDirectoryHandle;
}

/** One recently opened collection (`RecentCollection` of the UI). */
export interface RecentEntry {
  id: string;
  name: string;
  location: string;
  /** Unix milliseconds. */
  openedAt: number;
}

function request<T>(r: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    r.onsuccess = () => resolve(r.result);
    r.onerror = () => reject(r.error);
  });
}

async function db(): Promise<IDBDatabase> {
  const open = indexedDB.open(DB, 1);
  open.onupgradeneeded = () => open.result.createObjectStore(STORE);
  return request(open);
}

async function withStore<T>(mode: IDBTransactionMode, f: (s: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  const d = await db();
  try {
    return await request(f(d.transaction(STORE, mode).objectStore(STORE)));
  } finally {
    d.close();
  }
}

export async function lastFolder(): Promise<RememberedFolder | null> {
  try {
    const v = (await withStore("readonly", (s) => s.get(LAST))) as RememberedFolder | undefined;
    return v && typeof v.id === "string" && v.handle ? v : null;
  } catch {
    return null;
  }
}

/** All folders we gave an id, so reopening the same folder keeps its id (and its secrets). */
async function knownFolders(): Promise<RememberedFolder[]> {
  try {
    return ((await withStore("readonly", (s) => s.getAll())) as RememberedFolder[]).filter(
      (v) => v && typeof v.id === "string" && v.handle,
    );
  } catch {
    return [];
  }
}

/** The id for `handle`: the remembered one if this folder was opened before, else a new one.
 *  Also makes it the last opened folder. */
export async function rememberFolder(handle: FileSystemDirectoryHandle): Promise<string> {
  let id: string | null = null;
  for (const known of await knownFolders()) {
    if (await known.handle.isSameEntry(handle).catch(() => false)) {
      id = known.id;
      break;
    }
  }
  id ??= crypto.randomUUID();
  const entry: RememberedFolder = { id, handle };
  try {
    await withStore("readwrite", (s) => s.put(entry, `folder:${id}`));
    await withStore("readwrite", (s) => s.put(entry, LAST));
  } catch {
    // Not remembered; the folder still opens.
  }
  return id;
}

/** The folder we gave `id`, or `null`. */
export async function folderById(id: string): Promise<RememberedFolder | null> {
  try {
    const v = (await withStore("readonly", (s) => s.get(`folder:${id}`))) as RememberedFolder | undefined;
    return v && v.id === id && v.handle ? v : null;
  } catch {
    return null;
  }
}

function isEntry(v: unknown): v is RecentEntry {
  const e = v as RecentEntry;
  return (
    typeof v === "object" &&
    v !== null &&
    typeof e.id === "string" &&
    typeof e.name === "string" &&
    typeof e.location === "string" &&
    typeof e.openedAt === "number"
  );
}

/** Recently opened collections, most recent first (empty if storage is unavailable). */
export async function recentEntries(): Promise<RecentEntry[]> {
  try {
    const v: unknown = await withStore("readonly", (s) => s.get(RECENT));
    return Array.isArray(v) ? v.filter(isEntry).slice(0, MAX_RECENT) : [];
  } catch {
    return [];
  }
}

/** Puts `entry` at the top of the recent list. */
export async function recordRecent(entry: RecentEntry): Promise<void> {
  const list = [entry, ...(await recentEntries()).filter((e) => e.id !== entry.id)].slice(0, MAX_RECENT);
  try {
    await withStore("readwrite", (s) => s.put(list, RECENT));
  } catch {
    // Not remembered.
  }
}

/** Drops `id` from the recent list (its folder is gone). The folder keeps its id. */
export async function forgetRecent(id: string): Promise<void> {
  const list = (await recentEntries()).filter((e) => e.id !== id);
  try {
    await withStore("readwrite", (s) => s.put(list, RECENT));
  } catch {
    // Nothing to forget.
  }
}

/** Wave 14: the folders in the sidebar (root ids, in order) and the active collection. */
export interface RememberedRoots {
  roots: string[];
  active: string | null;
}

function isRoots(v: unknown): v is RememberedRoots {
  const r = v as RememberedRoots;
  return (
    typeof v === "object" &&
    v !== null &&
    Array.isArray(r.roots) &&
    r.roots.every((x) => typeof x === "string") &&
    (r.active === null || typeof r.active === "string")
  );
}

/** The folders in the sidebar as last left; `null` if none were remembered. */
export async function rememberedRoots(): Promise<RememberedRoots | null> {
  try {
    const v: unknown = await withStore("readonly", (s) => s.get(ROOTS));
    return isRoots(v) && v.roots.length > 0 ? v : null;
  } catch {
    return null;
  }
}

/** Remembers the folders in the sidebar and the active collection. */
export async function rememberRoots(roots: RememberedRoots): Promise<void> {
  try {
    await withStore("readwrite", (s) => s.put(roots, ROOTS));
  } catch {
    // Not remembered; the sidebar still works.
  }
}
