// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Recent collections of the web app (directory handles in IndexedDB, ./fs/recent.ts).
// `open(id, false)` succeeds only if the browser still grants access without asking (Chrome's
// "allow on every visit"); `open(id, true)` runs from a click and may show the browser's
// permission prompt. A folder that is gone (deleted, moved) is dropped from the list.
// Owner: wave 4, strand C.

import type { CollectionInfo, RecentCollection } from "@requesttailor/ui";

import { folderById, forgetRecent, recentEntries, recordRecent } from "./fs/recent";
import type { WebHost } from "./web-backend";

export interface WebRecent {
  /** Called by the backend with every collection it opened. */
  opened(info: CollectionInfo, handle: FileSystemDirectoryHandle): void;
  list(): Promise<RecentCollection[]>;
  open(id: string, interactive: boolean): Promise<CollectionInfo | null>;
}

type Permission = "granted" | "denied" | "prompt";

async function permission(handle: FileSystemDirectoryHandle, interactive: boolean): Promise<Permission> {
  // Browsers without the permission API (OPFS-only) grant what the handle allows.
  if (!handle.queryPermission) return "granted";
  const now = (await handle.queryPermission({ mode: "readwrite" }).catch(() => "denied")) as Permission;
  if (now !== "prompt" || !interactive || !handle.requestPermission) return now;
  return (await handle.requestPermission({ mode: "readwrite" }).catch(() => "denied")) as Permission;
}

/** Whether the folder behind `handle` still exists (reading it fails with NotFoundError). */
async function exists(handle: FileSystemDirectoryHandle): Promise<boolean> {
  try {
    const entries = handle.keys();
    await entries.next();
    return true;
  } catch (e) {
    if (e instanceof DOMException && e.name === "NotFoundError") return false;
    throw e;
  }
}

export function webRecent(host: WebHost): WebRecent {
  // IndexedDB updates run one after another, so quick opens never lose an entry.
  let writes: Promise<void> = Promise.resolve();
  const queue = (f: () => Promise<void>) => {
    writes = writes.then(f, f);
    return writes;
  };

  return {
    opened: (info) => {
      void queue(() =>
        recordRecent({ id: info.id, name: info.name, location: info.location, openedAt: Date.now() }),
      );
    },

    list: async () => {
      await writes;
      return recentEntries();
    },

    open: async (id, interactive) => {
      await writes;
      if (!(await recentEntries()).some((e) => e.id === id)) return null;
      const folder = await folderById(id);
      if (!folder) {
        await queue(() => forgetRecent(id));
        return null;
      }
      // Not granted (yet): at start the start screen offers the folder; after a click the
      // user declined.
      if ((await permission(folder.handle, interactive)) !== "granted") return null;
      if (!(await exists(folder.handle))) {
        await queue(() => forgetRecent(id));
        return null;
      }
      // Without a click this is the start-up reopen: it must not replace a collection the
      // user opened while it was on its way. Wave 14: a folder of the remembered sidebar
      // brings back all of its folders (those the browser still grants without asking).
      return host.openRecentRoot
        ? host.openRecentRoot(id, folder.handle, interactive)
        : host.openHandle(folder.handle, { unlessOpen: !interactive });
    },
  };
}
