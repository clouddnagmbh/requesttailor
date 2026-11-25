// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ConsumersApi` of the web app (wave 19, strand F), over rt-wasm (`crates/rt-wasm/src/consumers_js.rs`).
// The browser has no paths and no clones: the consumers listed in the collection's
// `opencollection.yml` are read from the collection's own Git repository (the folder the browser
// was given), so only a consumer whose path stays inside it can be verified; any other one is
// reported as such, never skipped. A run goes out like a send (the companion when paired, the same
// services, so the send guard answers per request). Secret values of a consumer are not available
// here: a secret without a value stays unresolved. Scripts of a consumer run only where the
// collection's own trust says so.

import type {
  ConsumerRef,
  ConsumerReport,
  ConsumerResult,
  ConsumerVerifyRequest,
  ConsumersApi,
  SendInput,
} from "@requesttailor/ui";

import type { RunContext } from "./runner";
import { toBackendError, type CoreModule } from "./wasm";

/** What rt-wasm exports for consumers (`consumers_js.rs`). */
export interface CoreConsumersModule {
  consumersList(opencollectionYml: string): ConsumerRef[];
  consumersRun(
    consumer: ConsumerRef,
    files: { path: string; text: string }[],
    request: ConsumerVerifyRequest,
    companion: RunContext["companion"],
    services: RunContext["services"],
    cancelled: () => boolean,
  ): Promise<ConsumerResult>;
}

export interface ConsumersHost {
  /** The Git repository of the active collection and the collection's folder in it. */
  repository(): { handle: FileSystemDirectoryHandle; scope: string } | null;
  core(): Promise<CoreModule>;
  sendContext(input: SendInput): Promise<RunContext>;
}

/** The most files and bytes one consumer may bring into the page. */
const MAX_FILES = 5_000;
const MAX_BYTES = 64 * 1024 * 1024;

/** `scope/source` as a path inside the repository, or `null` when it leaves it. */
export function inside(scope: string, source: string, path?: string): string[] | null {
  const parts = scope.split("/").filter(Boolean);
  for (const p of `${source}/${path ?? ""}`.split(/[\\/]+/)) {
    if (p === "" || p === ".") continue;
    if (p === "..") {
      if (parts.length === 0) return null;
      parts.pop();
    } else {
      parts.push(p);
    }
  }
  return parts;
}

async function dirAt(root: FileSystemDirectoryHandle, parts: string[]): Promise<FileSystemDirectoryHandle> {
  let dir = root;
  for (const p of parts) dir = await dir.getDirectoryHandle(p);
  return dir;
}

/** The `*.yml` files of a collection folder as text, as `FileSet::from_dir` reads them. */
async function readCollection(dir: FileSystemDirectoryHandle): Promise<{ path: string; text: string }[]> {
  const out: { path: string; text: string }[] = [];
  let bytes = 0;
  const walk = async (d: FileSystemDirectoryHandle, prefix: string): Promise<void> => {
    for await (const handle of d.values()) {
      const name = handle.name;
      if (name.startsWith(".") || name === "node_modules") continue;
      if (handle.kind === "directory") {
        await walk(handle as FileSystemDirectoryHandle, `${prefix}${name}/`);
      } else if (name.endsWith(".yml")) {
        const text = await (await (handle as FileSystemFileHandle).getFile()).text();
        bytes += text.length;
        if (out.length >= MAX_FILES || bytes > MAX_BYTES) throw new Error("the consumer collection is too large to read here");
        out.push({ path: `${prefix}${name}`, text });
      }
    }
  };
  await walk(dir, "");
  return out;
}

export function webConsumers(host: ConsumersHost): ConsumersApi {
  const module = async () => (await host.core()) as unknown as CoreConsumersModule;
  async function guard<T>(f: () => Promise<T>): Promise<T> {
    try {
      return await f();
    } catch (e) {
      throw toBackendError(e);
    }
  }
  const listed = async (): Promise<ConsumerRef[]> => {
    const repo = host.repository();
    if (!repo) return [];
    try {
      const dir = await dirAt(repo.handle, repo.scope.split("/").filter(Boolean));
      const text = await (await (await dir.getFileHandle("opencollection.yml")).getFile()).text();
      return (await module()).consumersList(text);
    } catch (e) {
      if (e instanceof DOMException && e.name === "NotFoundError") return [];
      throw e;
    }
  };
  let busy = false;
  return {
    list: () => guard(listed),
    verify: (request) =>
      guard(async () => {
        if (busy) throw new Error("A consumer verification is already in progress.");
        busy = true;
        try {
          const repo = host.repository();
          const all = await listed();
          const only = request.only ?? [];
          const unknown = only.find((n) => !all.some((c) => c.name === n));
          if (unknown) throw new Error(`no consumer named "${unknown}" (listed: ${all.map((c) => c.name).join(", ")})`);
          const chosen = all.filter((c) => only.length === 0 || only.includes(c.name));
          const core = await module();
          const report: ConsumerReport = { consumers: [] };
          const fail = (c: ConsumerRef, error: string): ConsumerResult => ({
            name: c.name,
            requests: 0,
            failures: [],
            error,
          });
          for (const c of chosen) {
            let result: ConsumerResult;
            const parts = repo ? inside(repo.scope, c.source, c.path) : null;
            if (!repo || !parts || /^[a-z][a-z0-9+.-]*:\/\//i.test(c.source) || c.source.includes("@")) {
              result = fail(
                c,
                `consumer ${c.name}: the browser can only read consumers inside this collection's repository, not "${c.source}"`,
              );
            } else {
              try {
                const files = await readCollection(await dirAt(repo.handle, parts));
                if (!files.some((f) => f.path === "opencollection.yml")) {
                  throw new Error(`no opencollection.yml in ${parts.join("/")}`);
                }
                const context = await host.sendContext({ environment: null, vars: [] });
                result = await core.consumersRun(
                  c,
                  files,
                  request,
                  context.companion,
                  context.services,
                  () => false,
                );
              } catch (e) {
                result = fail(c, `consumer ${c.name}: ${toBackendError(e).message}`);
              }
            }
            report.consumers.push(result);
            if (request.keep_going === false && (result.error || result.failures.length > 0)) break;
          }
          return report;
        } finally {
          busy = false;
        }
      }),
  };
}
