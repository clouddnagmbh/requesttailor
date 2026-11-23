// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `FolderScriptsApi` of the mock backend (wave 16): the scripts of the collection (`""`) and of
// its folders, kept in memory. Changes are unsaved until `save` (`dirty` lists the file), like
// `rt_app::Workspace::set_folder_scripts`.

import { BackendError, type FolderScriptsApi } from "./backend";
import { folderChain } from "./mock-variables";
import type { FileChange, RequestSpec, Script } from "./types";

export interface MockFolderScripts extends FolderScriptsApi {
  dirty(): string[];
  save(): FileChange[];
}

const fileOf = (folder: string) => (folder === "" ? "opencollection.yml" : `${folder}/folder.yml`);
const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

export function mockFolderScripts(requests: () => Map<string, RequestSpec>): MockFolderScripts {
  const scripts = new Map<string, Script[]>();
  const saved = new Map<string, Script[]>();
  const exists = (path: string) => {
    if (path === "") return true;
    for (const p of requests().keys()) if (folderChain(p).includes(path)) return true;
    return false;
  };
  const keys = () => new Set([...scripts.keys(), ...saved.keys()]);
  return {
    async get(path) {
      if (!exists(path)) throw new BackendError("folder-not-found", path);
      return structuredClone(scripts.get(path) ?? []);
    },
    async set(path, next) {
      if (!exists(path)) throw new BackendError("folder-not-found", path);
      scripts.set(path, structuredClone(next));
    },
    dirty() {
      return [...keys()].filter((k) => !same(scripts.get(k) ?? [], saved.get(k) ?? [])).map(fileOf).sort();
    },
    save() {
      const changes: FileChange[] = [];
      for (const k of keys()) {
        if (same(scripts.get(k) ?? [], saved.get(k) ?? [])) continue;
        saved.set(k, structuredClone(scripts.get(k) ?? []));
        changes.push({ path: fileOf(k), text: `# mock rendering (scripts)\n${JSON.stringify({ scripts: scripts.get(k) ?? [] }, null, 2)}\n`, deleted: false });
      }
      return changes;
    },
  };
}
