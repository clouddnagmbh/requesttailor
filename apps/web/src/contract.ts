// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ContractApi` of the web app (wave 12, strand F), over rt-wasm's contract functions
// (crates/rt-wasm/src/contract_js.rs): the same core steps as the desktop and `rt changelog`.
// This file only fetches what the core needs: the collection's files and the spec's bytes,
// from the open folder (saved files) or from Git without a checkout (`GitApi.treeAt`/`show`).
// In the browser the repository root is the collection folder, so a leading `/` in a spec path
// means the same as none.

import {
  BackendError,
  type ChangelogFacts,
  type ContractApi,
  type ContractReport,
  type ContractSpec,
  type SpecChanges,
} from "@requesttailor/ui";

import { readDirectory } from "./fs/directory";
import { directoryFs } from "./git/fs";
import { toBackendError, type CoreModule } from "./wasm";
import type { Wave12Host } from "./wave12";

/** What rt-wasm exports for the contract (`contract_js.rs`). */
export interface CoreContractModule {
  contractConfigured(files: [string, string][]): string | null | undefined;
  contractSpecInfo(spec: Uint8Array): { title: string; version: string };
  contractCheck(files: [string, string][], spec: Uint8Array): ContractReport;
  contractChanges(old: Uint8Array | null, next: Uint8Array | null, files: [string, string][]): SpecChanges;
  contractChangelog(
    oldFiles: [string, string][],
    oldSpec: Uint8Array | null,
    newFiles: [string, string][],
    newSpec: Uint8Array | null,
    from: string,
    to: string,
  ): string;
  contractChangelogFacts(
    oldFiles: [string, string][],
    oldSpec: Uint8Array | null,
    newFiles: [string, string][],
    newSpec: Uint8Array | null,
    from: string,
    to: string,
    commits: { oid: string; message: string; author: string }[],
  ): ChangelogFacts;
}

/** The commits of a range with their messages: the web Git adds this to `GitApi`
 *  (`ChangelogCommitSource`). Without it (a Git that cannot) only the summaries are known. */
type WithMessages = {
  changelogCommits?(from: string, to: string): Promise<{ oid: string; message: string; author: string }[]>;
};

/** Looked for, in this order, when `opencollection.yml` names no spec (`SPEC_CANDIDATES`). */
const CANDIDATES = ["openapi.yaml", "openapi.yml", "openapi.json"];

const NO_SPEC =
  "no OpenAPI spec found: add openapi.yaml next to the collection or at the repository root, or name it in opencollection.yml under extensions.requesttailor.contract.spec";

type Files = [string, string][];

interface Side {
  files: Files;
  /** The spec: its bytes, path and whether `opencollection.yml` named it. */
  spec: { bytes: Uint8Array; path: string; configured: boolean } | null;
}

/** The same rule as `rt_app::contract::safe_path`. */
function safePath(path: string): string {
  if (path === "" || path.includes("\\") || path.includes(":") || path.split("/").includes("..")) {
    throw new BackendError("format", `the spec path ${JSON.stringify(path)} must stay inside the collection or the repository`);
  }
  return path.replace(/^\/+/, "");
}

async function locate(
  core: CoreModule & CoreContractModule,
  files: Files,
  read: (path: string) => Promise<Uint8Array | null>,
): Promise<Side> {
  const configured = core.contractConfigured(files) ?? null; // wasm-bindgen gives `undefined` for `None`
  if (configured !== null) {
    const bytes = await read(safePath(configured));
    if (!bytes) {
      throw new BackendError("format", `the spec ${configured} named in opencollection.yml does not exist`);
    }
    return { files, spec: { bytes, path: configured, configured: true } };
  }
  for (const path of CANDIDATES) {
    const bytes = await read(path);
    if (bytes) return { files, spec: { bytes, path, configured: false } };
  }
  return { files, spec: null };
}

export function webContract(w: Wave12Host): ContractApi {
  const core = async () => (await w.host.deps.loadCore()) as CoreModule & CoreContractModule;

  async function saved(c: CoreModule & CoreContractModule): Promise<Side> {
    const handle = w.host.session().handle;
    const all = await readDirectory(handle);
    const dec = new TextDecoder();
    const files: Files = [...all]
      .filter(([p]) => p.endsWith(".yml"))
      .map(([p, b]) => [p, dec.decode(b)] as [string, string]);
    const { promises: fs } = directoryFs(handle);
    return locate(c, files, async (path) => {
      try {
        return (await fs.readFile(path)) as Uint8Array;
      } catch (e) {
        if ((e as { code?: string }).code === "ENOENT" || (e as { code?: string }).code === "EISDIR") return null;
        throw e;
      }
    });
  }

  async function at(c: CoreModule & CoreContractModule, rev: string): Promise<Side> {
    const tree = await w.git.treeAt(rev);
    const files: Files = Object.entries(tree.files);
    const enc = new TextEncoder();
    return locate(c, files, async (path) => {
      const text = await w.git.show(rev, path);
      return text === null ? null : enc.encode(text);
    });
  }

  async function guard<T>(f: () => Promise<T>): Promise<T> {
    try {
      return await f();
    } catch (e) {
      throw toBackendError(e);
    }
  }

  return {
    spec: () =>
      guard(async (): Promise<ContractSpec | null> => {
        const c = await core();
        const side = await saved(c);
        if (!side.spec) return null;
        const { title, version } = c.contractSpecInfo(side.spec.bytes);
        return { path: side.spec.path, title, version, configured: side.spec.configured };
      }),

    check: (rev) =>
      guard(async () => {
        const c = await core();
        const side = rev === null ? await saved(c) : await at(c, rev);
        if (!side.spec) throw new BackendError("format", NO_SPEC);
        return c.contractCheck(side.files, side.spec.bytes);
      }),

    changes: (from, to) =>
      guard(async () => {
        const c = await core();
        const [old, next] = [await at(c, from), await at(c, to)];
        return c.contractChanges(old.spec?.bytes ?? null, next.spec?.bytes ?? null, next.files);
      }),

    changelog: (from, to) =>
      guard(async () => {
        const c = await core();
        const [old, next] = [await at(c, from), await at(c, to)];
        return c.contractChangelog(old.files, old.spec?.bytes ?? null, next.files, next.spec?.bytes ?? null, from, to);
      }),

    // Wave 16, strand D: the same facts as `rt changelog` hands to a writer.
    changelogFacts: (from, to) =>
      guard(async () => {
        const c = await core();
        const [old, next] = [await at(c, from), await at(c, to)];
        const source = w.git as WithMessages;
        const commits = source.changelogCommits
          ? await source.changelogCommits(from, to)
          : (await w.git.commits(from, to, 10000)).map((x) => ({ oid: x.oid, message: x.summary, author: x.author }));
        return c.contractChangelogFacts(
          old.files,
          old.spec?.bytes ?? null,
          next.files,
          next.spec?.bytes ?? null,
          from,
          to,
          commits,
        );
      }),

    // Wave 17, strand A: writes the spec through the web Git scope, unsaved.
    stageSpec: () =>
      Promise.reject(new BackendError("unsupported", "Staging the spec is not implemented yet (wave 17, strand A).")),
  };
}
