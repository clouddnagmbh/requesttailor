// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The web app's GraphQL and Protobuf contracts (wave 18, strand H) over the real core (rt-wasm,
// built with `node scripts/build-wasm.mjs`) and a real Git repository on Node's fs: the same
// fixture and the same answers as crates/rt-format/tests/contract_{graphql,protobuf}.rs and
// crates/rt-cli/tests/changelog_schemas.rs. Skipped when the core is not built.

import * as nodeFs from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import type { GitApi } from "@requesttailor/ui";

import { webContract } from "./contract";
import type { FsClient } from "./git/fs";
import * as repoOps from "./git/repo";
import type { Repo } from "./git/repo";
import * as rev from "./git/revisions";
import type { Wave12Host } from "./wave12";

const WASM = join(__dirname, "../public/wasm");
const built = nodeFs.existsSync(join(WASM, "rt_wasm_bg.wasm"));
const FIXTURES = join(__dirname, "../../../testdata/contract");
const me = { name: "Test User", email: "test@example.com" };

const dirs: string[] = [];
afterEach(() => {
  for (const d of dirs.splice(0)) nodeFs.rmSync(d, { recursive: true, force: true });
});

async function core() {
  const mod = (await import(/* @vite-ignore */ join(WASM, "rt_wasm.js"))) as {
    initSync(options: { module: Uint8Array }): unknown;
  };
  mod.initSync({ module: nodeFs.readFileSync(join(WASM, "rt_wasm_bg.wasm")) });
  return mod;
}

function put(dir: string, path: string, data: Buffer | string) {
  nodeFs.mkdirSync(join(dir, path, ".."), { recursive: true });
  nodeFs.writeFileSync(join(dir, path), data);
}

function walk(dir: string, prefix = ""): string[] {
  return nodeFs.readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
    e.isDirectory() ? walk(join(dir, e.name), `${prefix}${e.name}/`) : [`${prefix}${e.name}`],
  );
}

/** The `schemas-collection` fixture with schema versions 1, 2, ... one commit each. */
async function repoWith(versions: number[]): Promise<Repo> {
  const dir = nodeFs.mkdtempSync(join(tmpdir(), "rt-schemas-"));
  dirs.push(dir);
  const r: Repo = { fs: nodeFs as unknown as FsClient, dir, cache: {} };
  await repoOps.init(r);
  const base = join(FIXTURES, "schemas-collection");
  const collection = walk(base);
  for (const p of collection) put(dir, p, nodeFs.readFileSync(join(base, p)));
  for (const [i, n] of versions.entries()) {
    const files = [
      ["api/shop.graphql", join(FIXTURES, `graphql/shop-v${n}.graphql`)],
      ["protos/orders.proto", join(FIXTURES, `protobuf/v${n}/orders.proto`)],
      ["protos/common/money.proto", join(FIXTURES, `protobuf/v${n}/common/money.proto`)],
    ] as const;
    for (const [to, from] of files) put(dir, to, nodeFs.readFileSync(from));
    const paths = [...files.map(([to]) => to), ...(i === 0 ? collection : [])];
    await repoOps.commit(r, `schemas v${n}`, paths, me);
  }
  return r;
}

function api(r: Repo, loaded: Awaited<ReturnType<typeof core>>) {
  const git = {
    treeAt: (rv: string) => rev.treeAt(r, rv),
    show: (rv: string, p: string) => rev.show(r, rv, p),
    commits: (from: string | null, to: string, limit: number) => rev.commits(r, from, to, limit),
    changelogCommits: (from: string, to: string) => rev.changelogCommits(r, from, to),
  } as unknown as GitApi;
  const host = { deps: { loadCore: async () => loaded } } as unknown as Wave12Host["host"];
  return webContract({ host, git, sendContext: async () => Promise.reject(new Error("unused")) });
}

const breakingOps = (c: { changes: { breaking: boolean; operation: string | null }[] }) =>
  c.changes
    .filter((x) => x.breaking)
    .map((x) => x.operation)
    .sort();

describe.skipIf(!built)("web GraphQL and Protobuf contracts over the real core", () => {
  it("finds the same breaking GraphQL changes as the native core, with their requests", async () => {
    const c = api(await repoWith([1, 2, 3]), await core());
    const changes = await c.schemaChanges("graphql", "HEAD~2", "HEAD");
    expect(breakingOps(changes)).toHaveLength(16);
    expect(breakingOps(changes)).toContain("Product.legacyCode");
    const field = changes.changes.find((x) => x.operation === "Product.legacyCode");
    expect(field).toMatchObject({ kind: "field-removed", breaking: true, requests: ["graphql/products.yml"] });
    const grew = await c.schemaChanges("graphql", "HEAD~2", "HEAD~1");
    expect(grew.changes.length).toBeGreaterThan(0);
    expect(grew.changes.filter((x) => x.breaking)).toEqual([]);
  });

  it("finds the same breaking Protobuf changes as the native core, with their requests", async () => {
    const c = api(await repoWith([1, 2, 3]), await core());
    const changes = await c.schemaChanges("protobuf", "HEAD~2", "HEAD");
    expect(breakingOps(changes)).toHaveLength(13);
    const watch = changes.changes.find((x) => x.operation === "/shop.orders.Orders/Watch");
    expect(watch).toMatchObject({ kind: "operation-removed", breaking: true, requests: ["grpc/watch.yml"] });
    const grew = await c.schemaChanges("protobuf", "HEAD~2", "HEAD~1");
    expect(grew.changes.filter((x) => x.breaking)).toEqual([]);
  });

  it("writes the sections of rt changelog, also in the facts", async () => {
    const c = api(await repoWith([1, 3]), await core());
    const text = await c.changelog("HEAD~1", "HEAD");
    const [graphql, protobuf] = [text.indexOf("\n## GraphQL\n"), text.indexOf("\n## Protobuf\n")];
    expect(graphql).toBeGreaterThan(0);
    expect(protobuf).toBeGreaterThan(graphql);
    expect(text).toContain("(affects `graphql/products.yml`)");
    expect(text).toContain("(affects `grpc/watch.yml`)");
    const facts = await c.changelogFacts("HEAD~1", "HEAD");
    expect(facts.markdown).toBe(text);
  });

  it("says nothing when the schemas did not change, and names a schema that does not parse", async () => {
    const r = await repoWith([1]);
    put(r.dir, "note.txt", "x");
    await repoOps.commit(r, "note", ["note.txt"], me);
    const loaded = await core();
    const same = await api(r, loaded).changelog("HEAD~1", "HEAD");
    expect(same).not.toContain("## GraphQL");
    expect(same).not.toContain("## Protobuf");
    put(r.dir, "api/shop.graphql", "type Query { a: }");
    await repoOps.commit(r, "broken", ["api/shop.graphql"], me);
    await expect(api(r, loaded).schemaChanges("graphql", "HEAD~1", "HEAD")).rejects.toThrow(/api\/shop\.graphql/);
  });
});
