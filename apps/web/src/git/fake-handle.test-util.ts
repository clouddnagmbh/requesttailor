// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// An in-memory FileSystemDirectoryHandle for unit tests (Node has no File System Access
// API). It throws the same DOMExceptions Chrome does: NotFoundError, TypeMismatchError,
// InvalidModificationError.

type Node = MemDir | MemFile;

/** Distinct modification times, so two quick writes never look alike. */
let clock = Date.now();

class MemFile {
  readonly kind = "file" as const;
  data = new Uint8Array();
  lastModified = ++clock;
  constructor(readonly name: string) {}

  async getFile(): Promise<File> {
    return new File([this.data.slice()], this.name, { lastModified: this.lastModified });
  }

  async createWritable() {
    const chunks: Uint8Array[] = [];
    return {
      write: async (data: string | Uint8Array) => {
        chunks.push(typeof data === "string" ? new TextEncoder().encode(data) : new Uint8Array(data));
      },
      close: async () => {
        const out = new Uint8Array(chunks.reduce((n, c) => n + c.length, 0));
        let at = 0;
        for (const c of chunks) {
          out.set(c, at);
          at += c.length;
        }
        this.data = out;
        this.lastModified = ++clock;
      },
      abort: async () => undefined,
    };
  }
}

class MemDir {
  readonly kind = "directory" as const;
  readonly entries = new Map<string, Node>();
  constructor(readonly name: string) {}

  private get(name: string): Node {
    const e = this.entries.get(name);
    if (!e) throw new DOMException(`${name} not found`, "NotFoundError");
    return e;
  }

  async getDirectoryHandle(name: string, options?: { create?: boolean }): Promise<MemDir> {
    if (!this.entries.has(name) && options?.create) this.entries.set(name, new MemDir(name));
    const e = this.get(name);
    if (e.kind !== "directory") throw new DOMException(`${name} is a file`, "TypeMismatchError");
    return e;
  }

  async getFileHandle(name: string, options?: { create?: boolean }): Promise<MemFile> {
    if (!this.entries.has(name) && options?.create) this.entries.set(name, new MemFile(name));
    const e = this.get(name);
    if (e.kind !== "file") throw new DOMException(`${name} is a directory`, "TypeMismatchError");
    return e;
  }

  async removeEntry(name: string, options?: { recursive?: boolean }): Promise<void> {
    const e = this.get(name);
    if (e.kind === "directory" && e.entries.size > 0 && !options?.recursive) {
      throw new DOMException(`${name} is not empty`, "InvalidModificationError");
    }
    this.entries.delete(name);
  }

  async *values(): AsyncIterableIterator<Node> {
    yield* [...this.entries.values()];
  }
}

/** A new empty in-memory folder, typed as the browser's handle. */
export function memoryFolder(name = "collection"): FileSystemDirectoryHandle {
  return new MemDir(name) as unknown as FileSystemDirectoryHandle;
}

/** Writes `files` (path → text) into a memory folder. */
export async function fill(root: FileSystemDirectoryHandle, files: Record<string, string>): Promise<void> {
  for (const [path, text] of Object.entries(files)) {
    const parts = path.split("/");
    let dir = root;
    for (const p of parts.slice(0, -1)) dir = await dir.getDirectoryHandle(p, { create: true });
    const w = await (await dir.getFileHandle(parts[parts.length - 1], { create: true })).createWritable();
    await w.write(text);
    await w.close();
  }
}
