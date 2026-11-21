// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The web app's only file I/O, through the File System Access API: reading a collection folder
// into the core's file map and writing the `FileChange`s of a save.
//
// Same rules as the desktop app (apps/desktop/src-tauri/src/files.rs). Every file is read
// as bytes (collection YAML and body files), except:
//   - folders whose name starts with "." (.git, ...) and node_modules;
//   - folders with their own opencollection.yml (a separate, nested collection);
//   - files whose name starts with ".";
//   - files larger than MAX_FILE_BYTES.
// More than MAX_FILES files or MAX_TOTAL_BYTES bytes is rejected with a clear message.
// Writes go through `createWritable()`, which writes to a temporary file and replaces the
// target only on `close()` (atomic per file, like temp + rename on desktop).

import { BackendError, type FileChange } from "@requesttailor/ui";

export const MAX_FILE_BYTES = 16 * 1024 * 1024;
export const MAX_FILES = 20_000;
export const MAX_TOTAL_BYTES = 256 * 1024 * 1024;
export const COLLECTION_FILE = "opencollection.yml";

export function skipDir(name: string): boolean {
  return name.startsWith(".") || name === "node_modules";
}

export function skipFile(name: string, size: number): boolean {
  return name.startsWith(".") || size > MAX_FILE_BYTES;
}

async function hasFile(dir: FileSystemDirectoryHandle, name: string): Promise<boolean> {
  try {
    await dir.getFileHandle(name);
    return true;
  } catch {
    return false;
  }
}

function tooLarge(why: string): BackendError {
  return new BackendError(
    "shell",
    `This folder is too large for a collection (${why}); open the collection's own folder.`,
  );
}

/** Reads the collection folder `root` into a map from `/`-separated relative path to bytes. */
export async function readDirectory(root: FileSystemDirectoryHandle): Promise<Map<string, Uint8Array>> {
  const files = new Map<string, Uint8Array>();
  let total = 0;
  const walk = async (dir: FileSystemDirectoryHandle, prefix: string): Promise<void> => {
    const entries: FileSystemHandle[] = [];
    for await (const entry of dir.values()) entries.push(entry);
    entries.sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
    for (const entry of entries) {
      const rel = prefix ? `${prefix}/${entry.name}` : entry.name;
      if (entry.kind === "directory") {
        const sub = entry as FileSystemDirectoryHandle;
        if (!skipDir(entry.name) && !(await hasFile(sub, COLLECTION_FILE))) await walk(sub, rel);
        continue;
      }
      const file = await (entry as FileSystemFileHandle).getFile();
      if (skipFile(entry.name, file.size)) continue;
      if (files.size >= MAX_FILES) throw tooLarge(`more than ${MAX_FILES} files`);
      total += file.size;
      if (total > MAX_TOTAL_BYTES) throw tooLarge(`more than ${MAX_TOTAL_BYTES / 1024 / 1024} MiB of files`);
      files.set(rel, new Uint8Array(await file.arrayBuffer()));
    }
  };
  await walk(root, "");
  return files;
}

/** Splits a collection path into its parts, rejecting anything that could leave the root. */
export function pathParts(path: string): string[] {
  const parts = path.split("/");
  const bad =
    path === "" ||
    path.includes("\\") ||
    path.includes("\0") ||
    parts.some((p) => p === "" || p === "." || p === "..");
  if (bad) throw new BackendError("shell", `invalid collection path ${JSON.stringify(path)}`);
  return parts;
}

/** Writes `changes` below `root`, creating folders as needed. All paths are checked before
 *  anything is written. */
export async function writeChanges(root: FileSystemDirectoryHandle, changes: FileChange[]): Promise<void> {
  const targets = changes.map((c) => ({ parts: pathParts(c.path), text: c.text }));
  for (const { parts, text } of targets) {
    let dir = root;
    for (const part of parts.slice(0, -1)) dir = await dir.getDirectoryHandle(part, { create: true });
    const file = await dir.getFileHandle(parts[parts.length - 1], { create: true });
    const writable = await file.createWritable();
    try {
      await writable.write(text);
      await writable.close();
    } catch (e) {
      await writable.abort().catch(() => undefined);
      throw e;
    }
  }
}
