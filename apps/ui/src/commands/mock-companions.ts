// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `CompanionApi` of the mock backend (wave 13): companion files in memory, staged until
// `save` like the real workspace (`MockBackend.dirty`/`save` include them). Same refusals as
// `rt_app::companions` (`*.yml`, paths that leave the folder).

import { BackendError, type CompanionApi } from "./backend";
import type { FileChange } from "./types";

export interface MockCompanions extends CompanionApi {
  /** Staged paths whose text differs from the saved one. */
  dirty(): string[];
  /** Writes the staged files and returns them. */
  save(): FileChange[];
}

function refused(path: string): boolean {
  return (
    path === "" ||
    path.endsWith(".yml") ||
    path.startsWith("/") ||
    /[\\:\0]/.test(path) ||
    path.split("/").some((p) => p === "" || p === "." || p === ".." || p.startsWith("."))
  );
}

/** `files`: every file "written" by the mock (`MockBackend.files`); saved companions go there
 *  too. */
export function mockCompanions(files: Map<string, string>, ensureOpen: () => void): MockCompanions {
  const saved = new Map<string, string>();
  const staged = new Map<string, string>();
  return {
    async list(prefix) {
      ensureOpen();
      const dir = prefix.replace(/\/+$/, "");
      return [...new Set([...saved.keys(), ...staged.keys()])]
        .filter((p) => dir === "" || p.startsWith(`${dir}/`))
        .sort();
    },
    async read(path) {
      ensureOpen();
      if (path.endsWith(".yml")) return null;
      return staged.get(path) ?? saved.get(path) ?? null;
    },
    async stage(path, text) {
      ensureOpen();
      if (refused(path)) throw new BackendError("shell", `cannot write ${JSON.stringify(path)} here`);
      staged.set(path, text);
    },
    dirty: () => [...staged].filter(([p, t]) => saved.get(p) !== t).map(([p]) => p),
    save() {
      const changes: FileChange[] = [];
      for (const [path, text] of staged) {
        if (saved.get(path) !== text) changes.push({ path, text, deleted: false });
        saved.set(path, text);
        files.set(path, text);
      }
      staged.clear();
      return changes;
    },
  };
}
