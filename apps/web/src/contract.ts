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
  type SchemaKind,
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
  /** Wave 18, strand H: the schema files still to read (`[path, text | null][]` is what was
   *  read so far); empty when everything is there. */
  contractSchemaWanted(kind: SchemaKind, files: [string, string][], read: SchemaRead): string[];
  contractSchemaChanges(
    kind: SchemaKind,
    oldFiles: [string, string][],
    oldRead: SchemaRead,
    newFiles: [string, string][],
    newRead: SchemaRead,
  ): SpecChanges;
  contractSchemaSection(
    kind: SchemaKind,
    oldFiles: [string, string][],
    oldRead: SchemaRead,
    newFiles: [string, string][],
    newRead: SchemaRead,
  ): string | null | undefined;
}

/** What was read of the schema files of one side: path (as the core asked for it) and its text,
 *  `null` where there is no such file. */
type SchemaRead = [string, string | null][];

/** Imports of imports of imports: far more than any real layout (as `rt_app::contract`). */
const SCHEMA_ROUNDS = 32;

/** The contract part of rt-wasm's `Workspace` (`contract_js.rs`). */
export interface CoreContractWorkspace {
  contractStageSpec(path: string, text: string): string;
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

  /** One side of a range for the schema comparison: the collection files and the schema files
   *  the core asked for, read with `git show` (in the browser a leading `/` is the collection
   *  folder, so it reads the same file). */
  async function schemaSide(
    c: CoreModule & CoreContractModule,
    kind: SchemaKind,
    rev: string,
  ): Promise<{ files: Files; read: SchemaRead }> {
    const files: Files = Object.entries((await w.git.treeAt(rev)).files);
    const read = new Map<string, string | null>();
    for (let round = 0; round < SCHEMA_ROUNDS; round++) {
      const wanted = c.contractSchemaWanted(kind, files, [...read]);
      if (wanted.length === 0) break;
      for (const path of wanted) read.set(path, await w.git.show(rev, path.replace(/^\/+/, "")));
    }
    return { files, read: [...read] };
  }

  async function schemaChanges(kind: SchemaKind, from: string, to: string): Promise<SpecChanges> {
    const c = await core();
    const [old, next] = [await schemaSide(c, kind, from), await schemaSide(c, kind, to)];
    return c.contractSchemaChanges(kind, old.files, old.read, next.files, next.read);
  }

  /** `text` with the GraphQL and Protobuf sections of the range after it, as `rt changelog`
   *  writes them (`rt_app::contract::changelog_with`). */
  async function withSchemaSections(text: string, from: string, to: string): Promise<string> {
    const c = await core();
    let out = text;
    for (const kind of ["graphql", "protobuf"] as const) {
      const [old, next] = [await schemaSide(c, kind, from), await schemaSide(c, kind, to)];
      const section = c.contractSchemaSection(kind, old.files, old.read, next.files, next.read);
      if (!section || section.trim() === "") continue;
      while (!out.endsWith("\n\n")) out += "\n";
      out += `${section.trim()}\n`;
    }
    return out;
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
        const text = c.contractChangelog(old.files, old.spec?.bytes ?? null, next.files, next.spec?.bytes ?? null, from, to);
        return withSchemaSections(text, from, to);
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
        const facts = c.contractChangelogFacts(
          old.files,
          old.spec?.bytes ?? null,
          next.files,
          next.spec?.bytes ?? null,
          from,
          to,
          commits,
        );
        // The schema sections are part of the text, as in `rt changelog`.
        return { ...facts, markdown: await withSchemaSections(facts.markdown, from, to) };
      }),

    // Wave 17, strand A: the spec the contract names (found as `spec()` does), staged in the
    // core workspace like a request edit; `save` writes it. The core refuses any other file
    // and a text that is no spec. In the browser a leading `/` is the collection folder.
    // Wave 18, strand H: the same files and rules as the desktop and `rt changelog`, over
    // rt-wasm.
    schemaChanges: (kind, from, to) => guard(() => schemaChanges(kind, from, to)),
    stageSpec: (text) =>
      guard(async () => {
        const c = await core();
        const side = await saved(c);
        if (!side.spec) throw new BackendError("format", NO_SPEC);
        const ws = w.host.session().ws as unknown as CoreContractWorkspace;
        return ws.contractStageSpec(side.spec.path, text);
      }),
  };
}
