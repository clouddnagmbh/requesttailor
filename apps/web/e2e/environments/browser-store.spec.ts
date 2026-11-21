// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Strand D acceptance: the web secret store keeps values encrypted at rest. Runs the real
// store (`apps/web/src/secrets/store.ts`, served by Vite) in the browser and then dumps every
// IndexedDB database, localStorage and sessionStorage to look for the plain text.

import { expect, test, type Page } from "@playwright/test";

import type { SecretsApi } from "@requesttailor/ui";

const PLAIN = "Pl4in-Secret-Value-öäü-✓";

type StoreModule = { openBrowserSecretStore(collectionId: string): Promise<SecretsApi> };

/** Everything in IndexedDB and Web Storage, with binary data as hex and as decoded text. */
async function dumpStorage(page: Page): Promise<string> {
  return page.evaluate(async () => {
    const hex = (b: Uint8Array) => [...b].map((x) => x.toString(16).padStart(2, "0")).join("");
    const show = (v: unknown): unknown => {
      if (v instanceof CryptoKey) return { cryptoKey: v.algorithm, extractable: v.extractable };
      if (v instanceof ArrayBuffer) v = new Uint8Array(v);
      if (ArrayBuffer.isView(v)) {
        const b = new Uint8Array(v.buffer, v.byteOffset, v.byteLength);
        return { hex: hex(b), latin1: String.fromCharCode(...b), utf8: new TextDecoder().decode(b) };
      }
      if (Array.isArray(v)) return v.map(show);
      if (v && typeof v === "object") return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, show(x)]));
      return v;
    };
    const out: unknown[] = [];
    for (const info of await indexedDB.databases()) {
      const db = await new Promise<IDBDatabase>((res, rej) => {
        const r = indexedDB.open(info.name!);
        r.onsuccess = () => res(r.result);
        r.onerror = () => rej(r.error);
      });
      for (const name of db.objectStoreNames) {
        const store = db.transaction(name, "readonly").objectStore(name);
        const [keys, values] = await Promise.all(
          [store.getAllKeys(), store.getAll()].map(
            (r) =>
              new Promise<unknown[]>((res, rej) => {
                r.onsuccess = () => res(r.result as unknown[]);
                r.onerror = () => rej(r.error);
              }),
          ),
        );
        out.push({ db: info.name, store: name, keys: show(keys), values: show(values) });
      }
      db.close();
    }
    const web = (s: Storage) => Object.fromEntries(Object.keys(s).map((k) => [k, s.getItem(k)]));
    out.push({ localStorage: web(localStorage), sessionStorage: web(sessionStorage) });
    return JSON.stringify(out);
  });
}

test("browser secret store: set, get, has, delete per collection", async ({ page }) => {
  await page.goto("/?backend=mock");
  const result = await page.evaluate(async (plain) => {
    const { openBrowserSecretStore } = (await import(/* @vite-ignore */ `${"/src/secrets/store.ts"}`)) as StoreModule;
    const a = await openBrowserSecretStore("collection-a");
    const b = await openBrowserSecretStore("collection-b");
    const steps: unknown[] = [];
    steps.push(await a.has("Local", "token"), await a.get("Local", "token"));
    await a.set("Local", "token", plain);
    steps.push(await a.has("Local", "token"), await a.get("Local", "token"));
    // Other collection, other environment: separate slots.
    steps.push(await b.get("Local", "token"), await a.get("PRD", "token"));
    await a.set("Local", "token", "second");
    steps.push(await a.get("Local", "token"));
    await a.delete("Local", "token");
    steps.push(await a.has("Local", "token"), await a.get("Local", "token"));
    await a.delete("Local", "token"); // deleting a missing value is fine
    return steps;
  }, PLAIN);
  expect(result).toEqual([false, null, true, PLAIN, null, null, "second", false, null]);
});

test("browser secret store: values are encrypted at rest with a non-extractable key", async ({ page }) => {
  await page.goto("/?backend=mock");
  const info = await page.evaluate(async (plain) => {
    const { openBrowserSecretStore } = (await import(/* @vite-ignore */ `${"/src/secrets/store.ts"}`)) as StoreModule;
    const s = await openBrowserSecretStore("collection-x");
    await s.set("Local", "token", plain);
    await s.set("Local", "same", plain);
    return { roundTrip: await s.get("Local", "token") };
  }, PLAIN);
  expect(info.roundTrip).toBe(PLAIN);

  const dump = await dumpStorage(page);
  expect(dump).toContain("requesttailor-secrets");
  expect(dump).toContain("collection-x");
  expect(dump).not.toContain(PLAIN);
  expect(dump).not.toContain(JSON.stringify(PLAIN).slice(1, -1));
  expect(dump).not.toContain(Buffer.from(PLAIN, "utf8").toString("hex"));

  // The key is a non-extractable AES-GCM 256 key; each value has its own 96-bit IV.
  const key = await page.evaluate(async () => {
    const db = await new Promise<IDBDatabase>((res, rej) => {
      const r = indexedDB.open("requesttailor-secrets");
      r.onsuccess = () => res(r.result);
      r.onerror = () => rej(r.error);
    });
    const get = <T>(store: string, k: IDBValidKey) =>
      new Promise<T>((res, rej) => {
        const r = db.transaction(store, "readonly").objectStore(store).get(k);
        r.onsuccess = () => res(r.result as T);
        r.onerror = () => rej(r.error);
      });
    const keys = await new Promise<CryptoKey[]>((res, rej) => {
      const r = db.transaction("keys", "readonly").objectStore("keys").getAll();
      r.onsuccess = () => res(r.result as CryptoKey[]);
      r.onerror = () => rej(r.error);
    });
    const a = await get<{ iv: Uint8Array }>("values", ["collection-x", "Local", "token"]);
    const b = await get<{ iv: Uint8Array }>("values", ["collection-x", "Local", "same"]);
    let exported = "no";
    try {
      await crypto.subtle.exportKey("raw", keys[0]);
      exported = "yes";
    } catch {
      /* expected */
    }
    db.close();
    return {
      count: keys.length,
      algorithm: keys[0].algorithm,
      extractable: keys[0].extractable,
      exported,
      ivLength: a.iv.byteLength,
      sameIv: a.iv.toString() === b.iv.toString(),
    };
  });
  expect(key).toEqual({
    count: 1,
    algorithm: { name: "AES-GCM", length: 256 },
    extractable: false,
    exported: "no",
    ivLength: 12,
    sameIv: false,
  });
});

test("browser secret store: the key survives a reload; a moved ciphertext does not decrypt", async ({ page }) => {
  await page.goto("/?backend=mock");
  await page.evaluate(async (plain) => {
    const { openBrowserSecretStore } = (await import(/* @vite-ignore */ `${"/src/secrets/store.ts"}`)) as StoreModule;
    await (await openBrowserSecretStore("collection-r")).set("Local", "token", plain);
  }, PLAIN);
  await page.reload();
  const after = await page.evaluate(async () => {
    const { openBrowserSecretStore } = (await import(/* @vite-ignore */ `${"/src/secrets/store.ts"}`)) as StoreModule;
    const s = await openBrowserSecretStore("collection-r");
    const value = await s.get("Local", "token");
    // Copy the ciphertext to another slot by hand: the key path is bound as additional data.
    const db = await new Promise<IDBDatabase>((res, rej) => {
      const r = indexedDB.open("requesttailor-secrets");
      r.onsuccess = () => res(r.result);
      r.onerror = () => rej(r.error);
    });
    await new Promise<void>((res, rej) => {
      const tx = db.transaction("values", "readwrite");
      const st = tx.objectStore("values");
      const r = st.get(["collection-r", "Local", "token"]);
      r.onsuccess = () => st.put(r.result, ["collection-r", "Local", "other"]);
      tx.oncomplete = () => res();
      tx.onerror = () => rej(tx.error);
    });
    db.close();
    let moved: string;
    try {
      moved = `decrypted: ${await s.get("Local", "other")}`;
    } catch (e) {
      moved = `${(e as Error).name}: ${(e as Error).message}`;
    }
    return { value, moved };
  });
  expect(after.value).toBe(PLAIN);
  expect(after.moved).toMatch(/^SecretStorageError: .*cannot be decrypted/);
});
