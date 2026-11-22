// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The file system isomorphic-git works on: the collection's FileSystemDirectoryHandle behind
// the promise-based `fs` API isomorphic-git expects (docs/fs.md of isomorphic-git: readFile,
// writeFile, unlink, readdir, mkdir, rmdir, stat, lstat, readlink, symlink, plus `rm`).
//
// - Paths are `/`-separated and relative to the handle; a leading `/` is allowed (Git ops run
//   with `dir: "/"`). `..` never leaves the handle: it is rejected.
// - Errors carry Node's `code` (`ENOENT`, `ENOTDIR`, `EISDIR`, `EEXIST`, `ENOTEMPTY`), which
//   is how isomorphic-git tells "missing" from real failures.
// - The browser has no file modes, inodes or symbolic links: files are 0o100644, folders
//   0o40000, `mtimeMs` is the file's `lastModified`; `readlink`/`symlink` fail with `ENOTSUP`.
// - Folder handles are cached per adapter; create one adapter per Git operation.

/** What `stat` returns (the fields isomorphic-git reads). */
export interface FsStats {
  type: "file" | "dir";
  mode: number;
  size: number;
  ino: number;
  dev: number;
  uid: number;
  gid: number;
  mtimeMs: number;
  ctimeMs: number;
  isFile(): boolean;
  isDirectory(): boolean;
  isSymbolicLink(): boolean;
}

type Encoding = "utf8" | { encoding?: string | null; mode?: number } | undefined | null;

export interface PromiseFs {
  readFile(path: string, options?: Encoding): Promise<Uint8Array | string>;
  writeFile(path: string, data: Uint8Array | string, options?: Encoding): Promise<void>;
  unlink(path: string): Promise<void>;
  readdir(path: string): Promise<string[]>;
  mkdir(path: string): Promise<void>;
  rmdir(path: string): Promise<void>;
  rm(path: string, options?: { recursive?: boolean }): Promise<void>;
  stat(path: string): Promise<FsStats>;
  lstat(path: string): Promise<FsStats>;
  readlink(path: string): Promise<string>;
  symlink(target: string, path: string): Promise<void>;
}

/** isomorphic-git's `fs` argument. */
export interface FsClient {
  promises: PromiseFs;
}

export class FsError extends Error {
  constructor(
    readonly code: string,
    path: string,
    detail = "",
  ) {
    super(`${code}: ${detail || code.toLowerCase()}, '${path}'`);
    this.name = "FsError";
  }
}

const FILE_MODE = 0o100644;
const DIR_MODE = 0o40000;

function stats(type: "file" | "dir", size: number, mtimeMs: number): FsStats {
  return {
    type,
    mode: type === "file" ? FILE_MODE : DIR_MODE,
    size,
    ino: 0,
    dev: 0,
    uid: 0,
    gid: 0,
    mtimeMs,
    ctimeMs: mtimeMs,
    isFile: () => type === "file",
    isDirectory: () => type === "dir",
    isSymbolicLink: () => false,
  };
}

/** Path → parts below the root. */
export function splitPath(path: string): string[] {
  if (typeof path !== "string") throw new FsError("EINVAL", String(path), "path must be a string");
  const parts = path.split("/").filter((p) => p !== "" && p !== ".");
  if (parts.some((p) => p === ".." || p.includes("\\") || p.includes("\0"))) {
    throw new FsError("EACCES", path, "path leaves the folder");
  }
  return parts;
}

function domName(e: unknown): string | null {
  return e instanceof DOMException || (e instanceof Error && e.name !== "Error") ? (e as Error).name : null;
}

/** Translates a File System Access error into an `FsError`; `asDir`: the path was expected
 *  to be a folder. */
function translate(e: unknown, path: string, asDir: boolean): unknown {
  if (e instanceof FsError) return e;
  switch (domName(e)) {
    case "NotFoundError":
      return new FsError("ENOENT", path, "no such file or directory");
    case "TypeMismatchError":
      return new FsError(asDir ? "ENOTDIR" : "EISDIR", path, asDir ? "not a directory" : "is a directory");
    case "InvalidModificationError":
      return new FsError("ENOTEMPTY", path, "directory not empty");
    case "NotAllowedError":
    case "SecurityError":
      return new FsError("EACCES", path, "permission denied");
    default:
      return e;
  }
}

export function directoryFs(root: FileSystemDirectoryHandle): FsClient {
  const dirs = new Map<string, FileSystemDirectoryHandle>([["", root]]);

  /** The folder at `parts` (no creation). */
  async function dirAt(parts: string[], path: string): Promise<FileSystemDirectoryHandle> {
    const key = parts.join("/");
    const cached = dirs.get(key);
    if (cached) return cached;
    const parent = await dirAt(parts.slice(0, -1), path);
    try {
      const dir = await parent.getDirectoryHandle(parts[parts.length - 1]);
      dirs.set(key, dir);
      return dir;
    } catch (e) {
      throw translate(e, path, true);
    }
  }

  /** Parent folder and name of a path that is not the root. */
  async function locate(path: string): Promise<{ parent: FileSystemDirectoryHandle; name: string; parts: string[] }> {
    const parts = splitPath(path);
    if (parts.length === 0) throw new FsError("EPERM", path, "operation not permitted on the root");
    return { parent: await dirAt(parts.slice(0, -1), path), name: parts[parts.length - 1], parts };
  }

  async function fileAt(path: string): Promise<FileSystemFileHandle> {
    const { parent, name } = await locate(path);
    try {
      return await parent.getFileHandle(name);
    } catch (e) {
      throw translate(e, path, false);
    }
  }

  function forget(prefix: string) {
    for (const key of [...dirs.keys()]) {
      if (key === prefix || key.startsWith(`${prefix}/`)) dirs.delete(key);
    }
  }

  const fs: PromiseFs = {
    async readFile(path, options) {
      const file = await (await fileAt(path)).getFile();
      const bytes = new Uint8Array(await file.arrayBuffer());
      const encoding = typeof options === "string" ? options : options?.encoding;
      return encoding === "utf8" || encoding === "utf-8" ? new TextDecoder().decode(bytes) : bytes;
    },

    async writeFile(path, data) {
      const { parent, name } = await locate(path);
      let handle: FileSystemFileHandle;
      try {
        handle = await parent.getFileHandle(name, { create: true });
      } catch (e) {
        throw translate(e, path, false);
      }
      const writable = await handle.createWritable();
      try {
        // A copy: `write` takes an ArrayBuffer-backed view, and Buffers may share a pool.
        await writable.write(typeof data === "string" ? data : new Uint8Array(data));
        await writable.close();
      } catch (e) {
        await writable.abort().catch(() => undefined);
        throw translate(e, path, false);
      }
    },

    async unlink(path) {
      const { parent, name } = await locate(path);
      try {
        await parent.getFileHandle(name);
        await parent.removeEntry(name);
      } catch (e) {
        throw translate(e, path, false);
      }
    },

    async readdir(path) {
      const parts = splitPath(path);
      let dir: FileSystemDirectoryHandle;
      try {
        dir = await dirAt(parts, path);
      } catch (e) {
        throw translate(e, path, true);
      }
      const names: string[] = [];
      for await (const entry of dir.values()) names.push(entry.name);
      return names;
    },

    async mkdir(path) {
      const { parent, name, parts } = await locate(path);
      const exists = await parent.getDirectoryHandle(name).then(
        () => true,
        (e: unknown) => domName(e) === "TypeMismatchError",
      );
      if (exists) throw new FsError("EEXIST", path, "file already exists");
      try {
        dirs.set(parts.join("/"), await parent.getDirectoryHandle(name, { create: true }));
      } catch (e) {
        throw translate(e, path, true);
      }
    },

    async rmdir(path) {
      const { parent, name, parts } = await locate(path);
      try {
        await parent.getDirectoryHandle(name);
        await parent.removeEntry(name);
      } catch (e) {
        throw translate(e, path, true);
      }
      forget(parts.join("/"));
    },

    async rm(path, options) {
      const { parent, name, parts } = await locate(path);
      try {
        await parent.removeEntry(name, { recursive: options?.recursive ?? false });
      } catch (e) {
        throw translate(e, path, false);
      }
      forget(parts.join("/"));
    },

    async stat(path) {
      const parts = splitPath(path);
      if (parts.length === 0) return stats("dir", 0, 0);
      const parent = await dirAt(parts.slice(0, -1), path);
      const name = parts[parts.length - 1];
      try {
        const file = await (await parent.getFileHandle(name)).getFile();
        return stats("file", file.size, file.lastModified);
      } catch (e) {
        if (domName(e) !== "TypeMismatchError") throw translate(e, path, false);
      }
      try {
        dirs.set(parts.join("/"), await parent.getDirectoryHandle(name));
        return stats("dir", 0, 0);
      } catch (e) {
        throw translate(e, path, true);
      }
    },

    lstat(path) {
      return fs.stat(path);
    },

    async readlink(path) {
      throw new FsError("ENOTSUP", path, "symbolic links are not supported in the browser");
    },

    async symlink(_target, path) {
      throw new FsError("ENOTSUP", path, "symbolic links are not supported in the browser");
    },
  };
  return { promises: fs };
}
