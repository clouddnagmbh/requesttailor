// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ImportApi` of the web app: the core's `Importer` converts; `write` asks for a folder,
// creates `plan.folder_name` in it, writes the files and opens it. Owner: wave 4, strand B.
// Wave 13: `writeInto` adds a plan to the open collection as a folder (the core's
// `add_to_workspace`, unsaved), `adopt` keeps a collection converted by an extension.

import { BackendError, type FileChange, type FolderImport, type ImportApi, type ImportPlan } from "@requesttailor/ui";

import { writeChanges } from "./fs/directory";
import { pluginsOf } from "./plugins";
import { toBackendError, type CoreImporter } from "./wasm";
import type { WebHost } from "./web-backend";

const NO_FOLDERS =
  "This browser cannot create folders. Use a current Chrome or Edge, or the RequestTailor desktop app.";

function isAbort(e: unknown): boolean {
  return e instanceof DOMException && e.name === "AbortError";
}

async function isEmpty(dir: FileSystemDirectoryHandle): Promise<boolean> {
  // `keys()` is async-iterable in every browser with File System Access.
  for await (const _ of (dir as unknown as { keys(): AsyncIterable<string> }).keys()) return false;
  return true;
}

export function webImports(host: WebHost): ImportApi {
  let importer: Promise<CoreImporter> | null = null;
  // Folder names of the plans, for `write` (the core keeps the files).
  const folders = new Map<number, string>();
  const core = () => (importer ??= host.deps.loadCore().then((m) => new m.Importer()));

  return {
    analyze: async (files, format) => {
      try {
        const plan = (await core()).analyze(files, format);
        folders.set(plan.id, plan.folder_name);
        return plan;
      } catch (e) {
        throw toBackendError(e);
      }
    },

    write: async (id) => {
      const name = folders.get(id);
      if (name === undefined) throw new BackendError("import", `import ${id} not found`);
      if (typeof window.showDirectoryPicker !== "function") {
        throw new BackendError("unsupported", NO_FOLDERS);
      }
      let parent: FileSystemDirectoryHandle;
      try {
        parent = await window.showDirectoryPicker({ id: "requesttailor-import", mode: "readwrite" });
      } catch (e) {
        if (isAbort(e)) return null;
        throw toBackendError(e);
      }
      try {
        let exists = true;
        await parent.getDirectoryHandle(name).catch(() => (exists = false));
        const dir = await parent.getDirectoryHandle(name, { create: true });
        if (exists && !(await isEmpty(dir))) {
          throw new BackendError("import", `${parent.name}/${name} already exists and is not empty; choose another folder`);
        }
        const imports = await core();
        await writeChanges(dir, imports.take(id));
        folders.delete(id);
        return await host.openHandle(dir);
      } catch (e) {
        throw toBackendError(e);
      }
    },

    discard: async (id) => {
      folders.delete(id);
      if (importer) (await importer).discard(id);
    },

    writeInto: async (id, name) => {
      const folder = folders.get(id);
      if (folder === undefined) throw new BackendError("import", `import ${id} not found`);
      const session = host.session();
      try {
        const files = (await core()).take(id);
        folders.delete(id);
        const ws = session.ws as typeof session.ws & { addImport(files: FileChange[], name: string): FolderImport };
        return ws.addImport(files, name?.trim() || folder);
      } catch (e) {
        throw toBackendError(e);
      }
    },

    adopt: async (imported) => {
      try {
        const imports = (await core()) as CoreImporter & { adopt(imported: unknown): ImportPlan };
        const plan = imports.adopt(imported);
        folders.set(plan.id, plan.folder_name);
        return plan;
      } catch (e) {
        throw toBackendError(e);
      }
    },

    // Wave 5, strand A: plugin importers through the web plugin host (./plugins.ts); the
    // result is checked and kept by the same core importer, so `write` works as for any import.
    analyzeWithPlugin: async (files, plugin, importerId) => {
      try {
        const reply = await pluginsOf(host).importReply(files, plugin, importerId);
        const imports = (await core()) as CoreImporter & { addPluginImport(reply: unknown): ImportPlan };
        const plan = imports.addPluginImport(reply);
        folders.set(plan.id, plan.folder_name);
        return plan;
      } catch (e) {
        throw toBackendError(e);
      }
    },
  };
}
