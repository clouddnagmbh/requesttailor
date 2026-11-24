// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `SnapshotApi` of the mock backend (wave 17, strand B), over the mock's companion files and
// requests with the model of the engine in `../snapshots/engine.ts`. Files are staged like
// the real workspace does (`MockBackend.dirty`/`save` include them); the frozen body is the
// stored response, which the mock has already masked.

import type { Assertion, AssertionResult, RequestSpec, Snapshot, SnapshotCheck, SnapshotEntry, SnapshotFreeze } from "./types";
import { BackendError, type CompanionApi, type SnapshotApi } from "./backend";
import {
  checkIgnore,
  companionPath,
  compareResponse,
  defaultValue,
  freezeResponse,
  parseSnapshot,
  snapshotText,
  summary,
  type MockResponseView,
} from "../snapshots/engine";

export const MATCHES_SNAPSHOT = "matchesSnapshot";

export interface MockSnapshotDeps {
  companions: CompanionApi;
  getRequest(path: string): Promise<RequestSpec>;
  updateRequest(path: string, spec: RequestSpec): Promise<void>;
  /** Every request path of the active collection. */
  requestPaths(): string[];
  /** A kept response; `undefined` when it is gone. */
  response(id: number): MockResponseView | undefined;
}

const assertionOf = (spec: RequestSpec) => spec.assertions.find((a) => a.operator.trim() === MATCHES_SNAPSHOT);

/** The companion file of the request, or `null` if it has neither the assertion nor a file. */
async function locate(deps: MockSnapshotDeps, path: string, spec: RequestSpec): Promise<string | null> {
  const a = assertionOf(spec);
  if (a) return companionPath(path, a.value.trim() || defaultValue(path));
  const file = companionPath(path, defaultValue(path));
  return (await deps.companions.read(file)) === null ? null : file;
}

/** Evaluates the snapshot assertions of a send (`mock.ts`); `read` gives the text of a file. */
export async function evaluateSnapshots(
  assertions: Assertion[],
  path: string,
  response: MockResponseView,
  read: (file: string) => Promise<string | null>,
): Promise<AssertionResult[]> {
  const out: AssertionResult[] = [];
  for (const a of assertions.filter((x) => x.enabled && x.operator.trim() === MATCHES_SNAPSHOT)) {
    const result: AssertionResult = { expression: a.expression, operator: a.operator, expected: a.value, actual: null, passed: false };
    const file = a.value.trim() || defaultValue(path);
    const text = await read(companionPath(path, file));
    if (text === null) {
      result.actual = `${file} is missing`;
      result.error = "no snapshot yet; run `rt snapshot update`";
    } else {
      try {
        const differences = compareResponse(parseSnapshot(text), response, []);
        result.passed = differences.length === 0;
        result.actual = result.passed ? "matches" : `${differences.length} difference(s)`;
        if (!result.passed) result.error = summary(differences);
      } catch (e) {
        result.error = `the snapshot ${file} cannot be read: ${e instanceof Error ? e.message : String(e)}`;
      }
    }
    out.push(result);
  }
  return out;
}

export function mockSnapshots(deps: MockSnapshotDeps): SnapshotApi {
  const read = async (path: string): Promise<{ snapshot: Snapshot; file: string } | null> => {
    const spec = await deps.getRequest(path);
    const file = await locate(deps, path, spec);
    if (file === null) return null;
    const text = await deps.companions.read(file);
    if (text === null) return null;
    try {
      return { snapshot: parseSnapshot(text), file };
    } catch (e) {
      throw new BackendError("shell", `the snapshot ${file} cannot be read: ${e instanceof Error ? e.message : String(e)}`);
    }
  };
  const response = (id: number): MockResponseView => {
    const r = deps.response(id);
    if (!r) throw new BackendError("response-not-found", String(id));
    return r;
  };
  return {
    async read(path) {
      return (await read(path))?.snapshot ?? null;
    },

    async freeze(path, responseId, options): Promise<SnapshotFreeze> {
      const spec = await deps.getRequest(path);
      const existing = await read(path);
      const content = freezeResponse(response(responseId), [], {
        keepEtag: (options?.keep_etag ?? false) || (existing ? "etag" in existing.snapshot.headers : false),
        ignore: existing?.snapshot.ignore ?? [],
      });
      const text = snapshotText(content);
      if (/bearer\s+[A-Za-z0-9._~+/=-]{24,}|"password"\s*:\s*"[^"*{][^"]{5,}"/i.test(text)) {
        throw new BackendError("secrets-found", `Commit refused: 1 credential in 1 file (${path}). Make it a secret variable.`);
      }
      const assertionAdded = assertionOf(spec) === undefined;
      let value: string;
      if (assertionAdded) {
        value = options?.file?.trim() || defaultValue(path);
        if (!value.endsWith(".snap.json") || value.startsWith("/") || value.includes("..")) {
          throw new BackendError("shell", `${JSON.stringify(value)} is not a snapshot file name (it must end in .snap.json, beside the request)`);
        }
        spec.assertions.push({ expression: "res", operator: MATCHES_SNAPSHOT, value, description: null, enabled: true });
      } else {
        value = assertionOf(spec)!.value.trim() || defaultValue(path);
      }
      const file = companionPath(path, value);
      await deps.companions.stage(file, text);
      if (assertionAdded) await deps.updateRequest(path, spec);
      return { request_path: path, snapshot: file, assertion_added: assertionAdded, content };
    },

    async setIgnore(path, ignore) {
      for (const p of ignore) {
        const problem = checkIgnore(p);
        if (problem) throw new BackendError("shell", `ignore path ${JSON.stringify(p)}: ${problem}`);
      }
      const found = await read(path);
      if (!found) throw new BackendError("shell", "the request has no snapshot yet");
      const snapshot = { ...found.snapshot, ignore: [...new Set(ignore)] };
      await deps.companions.stage(found.file, snapshotText(snapshot));
      return snapshot;
    },

    async check(path, responseId): Promise<SnapshotCheck> {
      const spec = await deps.getRequest(path);
      const file = await locate(deps, path, spec);
      const base = { request_path: path, differences: [] };
      if (file === null) return { ...base, snapshot: "", state: "none" };
      const found = await read(path);
      if (!found) return { ...base, snapshot: file, state: "missing" };
      const differences = compareResponse(found.snapshot, response(responseId), []);
      return { ...base, snapshot: file, state: differences.length === 0 ? "matches" : "differs", differences };
    },

    async list(): Promise<SnapshotEntry[]> {
      const out: SnapshotEntry[] = [];
      for (const path of deps.requestPaths()) {
        const spec = await deps.getRequest(path);
        const a = assertionOf(spec);
        if (!a) continue;
        const file = companionPath(path, a.value.trim() || defaultValue(path));
        out.push({ request_path: path, snapshot: file, exists: (await deps.companions.read(file)) !== null });
      }
      return out;
    },
  };
}
