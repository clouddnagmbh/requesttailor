// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Remembers the last opened folder: its directory handle and the collection id we gave it
// (random, stable per folder on this browser profile; used as `SecretKey.collection`).
// Directory handles can only be stored in IndexedDB. Storage may be unavailable (private
// windows, blocked site data): then nothing is remembered and every open shows the picker.

const DB = "requesttailor-web";
const STORE = "folders";
const LAST = "last";

export interface RememberedFolder {
  id: string;
  handle: FileSystemDirectoryHandle;
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
