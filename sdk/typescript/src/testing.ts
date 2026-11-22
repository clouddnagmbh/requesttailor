// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// A small plugin host for tests, on the JavaScript engine's own WebAssembly (Node, browsers).
// It follows `crates/rt-plugin/API.md`: it links only `rt.host` and the WASI subset, reads
// the manifest from the `rt-plugin` custom section, calls `rt_call` with JSON and answers
// host calls within the granted permissions. It is not the product host: no fuel limit, and
// `http` is answered synchronously by a test callback (the real host suspends the plugin).

import type {
  Call,
  HostCall,
  Manifest,
  NoteLevel,
  Permissions,
  PluginRequest,
  PluginResponse,
  Reply,
} from "./api.ts";
import { MANIFEST_SECTION } from "./api.ts";

/** The `wasi_snapshot_preview1` functions a plugin may import (API.md, "Binary interface"). */
export const WASI_SUBSET = [
  "fd_write",
  "clock_time_get",
  "random_get",
  "environ_get",
  "environ_sizes_get",
  "args_get",
  "args_sizes_get",
  "proc_exit",
] as const;

/** The exports every plugin has. */
export const REQUIRED_EXPORTS = ["memory", "rt_alloc", "rt_dealloc", "rt_call"] as const;

export interface TestHostOptions {
  /** What the user granted; default: nothing. */
  granted?: Permissions;
  /** The host of the request the hook runs for (`{request}` network entry). */
  requestHost?: string;
  /** Unix milliseconds for `now`; default `Date.now()`. */
  now?: () => number;
  /** Answers `http` calls that pass the permission check. Without it they fail. */
  http?: (request: PluginRequest, credentials: boolean) => PluginResponse;
}

export interface LogLine {
  level: NoteLevel;
  message: string;
}

/** What is wrong with a module's interface, or an empty list. Does not run it. */
export function checkInterface(module: WebAssembly.Module): string[] {
  const problems: string[] = [];
  const wasi = new Set<string>(WASI_SUBSET);
  for (const i of WebAssembly.Module.imports(module)) {
    const ok =
      (i.module === "rt" && i.name === "host" && i.kind === "function") ||
      (i.module === "wasi_snapshot_preview1" && wasi.has(i.name) && i.kind === "function");
    if (!ok) problems.push(`import not allowed: ${i.module}.${i.name} (${i.kind})`);
  }
  const exports = new Map(WebAssembly.Module.exports(module).map((e) => [e.name, e.kind]));
  for (const name of REQUIRED_EXPORTS) {
    const kind = name === "memory" ? "memory" : "function";
    if (exports.get(name) !== kind) problems.push(`missing export: ${name} (${kind})`);
  }
  const sections = WebAssembly.Module.customSections(module, MANIFEST_SECTION);
  if (sections.length !== 1) {
    problems.push(`expected one "${MANIFEST_SECTION}" custom section, found ${sections.length}`);
  }
  return problems;
}

/** The manifest of a module (custom section `rt-plugin`). */
export function readManifest(module: WebAssembly.Module): Manifest {
  const [section] = WebAssembly.Module.customSections(module, MANIFEST_SECTION);
  if (!section) throw new Error(`no "${MANIFEST_SECTION}" custom section`);
  return JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(section)) as Manifest;
}

interface Exports {
  memory: WebAssembly.Memory;
  rt_alloc(len: number): number;
  rt_dealloc(ptr: number, len: number): void;
  rt_call(ptr: number, len: number): bigint;
}

/** A loaded plugin. */
export class TestPlugin {
  readonly manifest: Manifest;
  readonly logs: LogLine[] = [];
  readonly hostCalls: HostCall[] = [];
  readonly storage = new Map<string, unknown>();
  /** Output of `fd_write` to fds 1 and 2. */
  readonly stdio: string[] = [];
  #exports!: Exports;
  readonly #options: TestHostOptions;

  private constructor(manifest: Manifest, options: TestHostOptions) {
    this.manifest = manifest;
    this.#options = options;
  }

  /** Compiles, checks and instantiates a plugin module. Throws on interface problems. */
  static async load(wasm: BufferSource, options: TestHostOptions = {}): Promise<TestPlugin> {
    const module = await WebAssembly.compile(wasm);
    const problems = checkInterface(module);
    if (problems.length > 0) throw new Error(`not a valid plugin: ${problems.join("; ")}`);
    const plugin = new TestPlugin(readManifest(module), options);
    const instance = await WebAssembly.instantiate(module, plugin.#imports());
    plugin.#exports = instance.exports as unknown as Exports;
    return plugin;
  }

  /** Runs one hook and returns its decoded reply. */
  call<T = unknown>(call: Call): Reply<T> {
    return JSON.parse(this.callRaw(JSON.stringify(call))) as Reply<T>;
  }

  /** Runs `rt_call` with any bytes (e.g. invalid JSON) and returns the reply text. */
  callRaw(json: string): string {
    const e = this.#exports;
    const input = new TextEncoder().encode(json);
    const ptr = e.rt_alloc(input.length);
    new Uint8Array(e.memory.buffer, ptr, input.length).set(input);
    const packed = e.rt_call(ptr, input.length);
    const [replyPtr, replyLen] = unpack(packed);
    // Copy before freeing; the memory may also have grown (new buffer) during the call.
    const reply = new TextDecoder().decode(
      new Uint8Array(e.memory.buffer, replyPtr, replyLen).slice(),
    );
    e.rt_dealloc(replyPtr, replyLen);
    e.rt_dealloc(ptr, input.length);
    return reply;
  }

  /** Current size of the plugin's linear memory in bytes. */
  memoryBytes(): number {
    return this.#exports.memory.buffer.byteLength;
  }

  #imports(): WebAssembly.Imports {
    const memory = () => this.#exports.memory;
    const view = () => new DataView(memory().buffer);
    const bytes = (ptr: number, len: number) => new Uint8Array(memory().buffer, ptr, len);
    return {
      rt: {
        host: (ptr: number, len: number): bigint => {
          const text = new TextDecoder().decode(bytes(ptr, len).slice());
          const reply = JSON.stringify(this.#answer(text));
          const out = new TextEncoder().encode(reply);
          const replyPtr = this.#exports.rt_alloc(out.length);
          bytes(replyPtr, out.length).set(out);
          return pack(replyPtr, out.length);
        },
      },
      wasi_snapshot_preview1: {
        fd_write: (fd: number, iovs: number, iovsLen: number, written: number): number => {
          if (fd !== 1 && fd !== 2) return 8; // EBADF
          let total = 0;
          let text = "";
          for (let i = 0; i < iovsLen; i++) {
            const base = view().getUint32(iovs + i * 8, true);
            const len = view().getUint32(iovs + i * 8 + 4, true);
            text += new TextDecoder().decode(bytes(base, len).slice());
            total += len;
          }
          this.stdio.push(text);
          view().setUint32(written, total, true);
          return 0;
        },
        clock_time_get: (_id: number, _precision: bigint, out: number): number => {
          view().setBigUint64(out, BigInt(this.#now()) * 1_000_000n, true);
          return 0;
        },
        random_get: (ptr: number, len: number): number => {
          crypto.getRandomValues(bytes(ptr, len));
          return 0;
        },
        environ_get: () => 0,
        environ_sizes_get: (count: number, size: number): number => {
          view().setUint32(count, 0, true);
          view().setUint32(size, 0, true);
          return 0;
        },
        args_get: () => 0,
        args_sizes_get: (count: number, size: number): number => {
          view().setUint32(count, 0, true);
          view().setUint32(size, 0, true);
          return 0;
        },
        proc_exit: (code: number): never => {
          throw new Error(`plugin called proc_exit(${code})`);
        },
      },
    };
  }

  #now(): number {
    return this.#options.now ? this.#options.now() : Date.now();
  }

  #answer(text: string): Reply<unknown> {
    let call: HostCall;
    try {
      call = JSON.parse(text) as HostCall;
    } catch (e) {
      return { error: `invalid host call: ${String(e)}` };
    }
    this.hostCalls.push(call);
    const granted = this.#options.granted ?? {};
    switch (call.op) {
      case "log":
        this.logs.push({ level: call.level, message: call.message });
        return { ok: null };
      case "now":
        return { ok: this.#now() };
      case "storage-get":
      case "storage-set":
      case "storage-delete":
        if (!granted.storage) return { error: "permission denied: storage" };
        if (call.op === "storage-get") return { ok: this.storage.get(call.key) ?? null };
        if (call.op === "storage-set") this.storage.set(call.key, call.value);
        else this.storage.delete(call.key);
        return { ok: null };
      case "http": {
        const host = hostOf(call.request.url);
        if (!host || !networkAllows(granted.network ?? [], host, this.#options.requestHost)) {
          return { error: "permission denied: network" };
        }
        if (!this.#options.http) return { error: "test host: no http handler" };
        try {
          return { ok: this.#options.http(call.request, call.credentials ?? false) };
        } catch (e) {
          return { error: String(e) };
        }
      }
      default:
        return { error: `unknown host call: ${(call as { op: string }).op}` };
    }
  }
}

function pack(ptr: number, len: number): bigint {
  return BigInt.asIntN(64, (BigInt(ptr >>> 0) << 32n) | BigInt(len >>> 0));
}

function unpack(packed: bigint): [number, number] {
  const u = BigInt.asUintN(64, packed);
  return [Number(u >> 32n), Number(u & 0xffff_ffffn)];
}

/** The host of a URL, lowercase, or `null`. */
export function hostOf(url: string): string | null {
  try {
    const host = new URL(url).hostname.toLowerCase();
    return host.startsWith("[") ? host.slice(1, -1) : host || null;
  } catch {
    return null;
  }
}

/** The `network` permission rules of API.md. */
export function networkAllows(entries: string[], host: string, requestHost?: string): boolean {
  const h = host.toLowerCase();
  return entries.some((entry) => {
    if (entry === "*") return true;
    if (entry === "{request}") return requestHost?.toLowerCase() === h;
    if (entry.startsWith("*.")) {
      const suffix = entry.slice(1).toLowerCase();
      return h.endsWith(suffix) && h.length > suffix.length;
    }
    return entry.toLowerCase() === h;
  });
}
