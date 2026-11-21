// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Secret values in the browser: encrypted (AES-GCM via WebCrypto) in IndexedDB, with a
// non-extractable key that never leaves this browser profile. Never in collection files,
// never in localStorage in clear text. Owner: wave 3, strand D.
//
// Layout (database `requesttailor-secrets`):
//   - store `keys`:   "aes-gcm-256" -> CryptoKey (structured clone, `extractable: false`).
//                     Generated once per browser profile.
//   - store `values`: [collectionId, environment, variable] -> { iv, data }.
//                     `iv` is a random 96-bit nonce per write; `data` is the AES-GCM
//                     ciphertext. The key path is bound as additional data, so a ciphertext
//                     cannot be moved to another variable.

import type { SecretsApi } from "@requesttailor/ui";

const DB_NAME = "requesttailor-secrets";
const DB_VERSION = 1;
const KEYS = "keys";
const VALUES = "values";
const KEY_ID = "aes-gcm-256";

type SlotKey = [collection: string, environment: string, variable: string];

interface Sealed {
  iv: Uint8Array<ArrayBuffer>;
  data: ArrayBuffer;
}

/** Storage failures, with a message the UI can show as is. */
export class SecretStorageError extends Error {
  constructor(message: string, options?: { cause?: unknown }) {
    super(message, options);
    this.name = "SecretStorageError";
  }
}

const detail = (e: unknown) =>
  e instanceof DOMException ? `${e.name}: ${e.message}` : e instanceof Error ? e.message : String(e);

function unavailable(e: unknown): SecretStorageError {
  return new SecretStorageError(
    "Secret storage is not available in this browser (private window, blocked site data or " +
      `storage full?). Secret values cannot be saved. (${detail(e)})`,
    { cause: e },
  );
}

const request = <T>(r: IDBRequest<T>) =>
  new Promise<T>((resolve, reject) => {
    r.onsuccess = () => resolve(r.result);
    r.onerror = () => reject(r.error);
  });

const done = (tx: IDBTransaction) =>
  new Promise<void>((resolve, reject) => {
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
    tx.onabort = () => reject(tx.error ?? new DOMException("transaction aborted", "AbortError"));
  });

function openDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    let r: IDBOpenDBRequest;
    try {
      if (typeof indexedDB === "undefined") throw new Error("IndexedDB is missing");
      r = indexedDB.open(DB_NAME, DB_VERSION);
    } catch (e) {
      reject(unavailable(e));
      return;
    }
    r.onupgradeneeded = () => {
      const db = r.result;
      if (!db.objectStoreNames.contains(KEYS)) db.createObjectStore(KEYS);
      if (!db.objectStoreNames.contains(VALUES)) db.createObjectStore(VALUES);
    };
    r.onsuccess = () => {
      const db = r.result;
      // Another tab upgrading the schema: let it, reopen on next use.
      db.onversionchange = () => {
        db.close();
        opened = null;
      };
      resolve(db);
    };
    r.onerror = () => reject(unavailable(r.error));
    r.onblocked = () => reject(unavailable(new Error("database is blocked by another tab")));
  });
}

async function loadOrCreateKey(db: IDBDatabase): Promise<CryptoKey> {
  const read = async () => {
    const tx = db.transaction(KEYS, "readonly");
    return (await request(tx.objectStore(KEYS).get(KEY_ID))) as CryptoKey | undefined;
  };
  const existing = await read();
  if (existing) return existing;
  if (!globalThis.crypto?.subtle) {
    throw new SecretStorageError("Secret storage needs WebCrypto (a secure context: https or localhost).");
  }
  const key = await crypto.subtle.generateKey({ name: "AES-GCM", length: 256 }, false, [
    "encrypt",
    "decrypt",
  ]);
  try {
    const tx = db.transaction(KEYS, "readwrite");
    // `add`, not `put`: if another tab won the race, keep its key and use that.
    tx.objectStore(KEYS).add(key, KEY_ID);
    await done(tx);
    return key;
  } catch (e) {
    const winner = await read();
    if (winner) return winner;
    throw unavailable(e);
  }
}

interface Opened {
  db: IDBDatabase;
  key: CryptoKey;
}

let opened: Promise<Opened> | null = null;

function open(): Promise<Opened> {
  if (!opened) {
    opened = (async () => {
      const db = await openDb();
      return { db, key: await loadOrCreateKey(db) };
    })();
    // A failed open (e.g. private mode) is retried on the next call.
    opened.catch(() => (opened = null));
  }
  return opened;
}

const encoder = new TextEncoder();
const additionalData = (slot: SlotKey) => encoder.encode(JSON.stringify(slot));

async function seal(key: CryptoKey, slot: SlotKey, value: string): Promise<Sealed> {
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const data = await crypto.subtle.encrypt(
    { name: "AES-GCM", iv, additionalData: additionalData(slot) },
    key,
    encoder.encode(value),
  );
  return { iv, data };
}

async function unseal(key: CryptoKey, slot: SlotKey, sealed: Sealed): Promise<string> {
  try {
    const plain = await crypto.subtle.decrypt(
      { name: "AES-GCM", iv: sealed.iv, additionalData: additionalData(slot) },
      key,
      sealed.data,
    );
    return new TextDecoder().decode(plain);
  } catch (e) {
    throw new SecretStorageError(
      `Secret "${slot[2]}" of environment "${slot[1]}" cannot be decrypted; set it again. (${detail(e)})`,
      { cause: e },
    );
  }
}

async function withStore<T>(mode: IDBTransactionMode, f: (store: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  const { db } = await open();
  try {
    const tx = db.transaction(VALUES, mode);
    const [result] = await Promise.all([request(f(tx.objectStore(VALUES))), done(tx)]);
    return result;
  } catch (e) {
    throw e instanceof SecretStorageError ? e : unavailable(e);
  }
}

/** Secret store for one collection (`collectionId` = `CollectionInfo.id`). */
export async function openBrowserSecretStore(collectionId: string): Promise<SecretsApi> {
  const { key } = await open();
  const slot = (environment: string, variable: string): SlotKey => [collectionId, environment, variable];

  return {
    has: async (environment, variable) =>
      (await withStore("readonly", (s) => s.count(slot(environment, variable)))) > 0,

    get: async (environment, variable) => {
      const k = slot(environment, variable);
      const sealed = (await withStore("readonly", (s) => s.get(k))) as Sealed | undefined;
      return sealed ? unseal(key, k, sealed) : null;
    },

    set: async (environment, variable, value) => {
      const k = slot(environment, variable);
      const sealed = await seal(key, k, value);
      await withStore("readwrite", (s) => s.put(sealed, k));
    },

    delete: async (environment, variable) => {
      await withStore("readwrite", (s) => s.delete(slot(environment, variable)));
    },
  };
}
