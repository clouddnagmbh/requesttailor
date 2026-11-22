// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Fetching a pull request's head in the web app (wave 12, strand E) against the local fake Git
// host (testdata/review/fake-host.mjs; needs `node` and `git`): the head and the target land in
// the review refs without a checkout, the token goes only to the review host, errors are
// readable. isomorphic-git talks to the fake host directly here (no companion in Node).

import { spawn, type ChildProcess } from "node:child_process";
import * as nodeFs from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";

import git from "isomorphic-git";
import nodeHttp from "isomorphic-git/http/node";
import { afterAll, beforeAll, describe, expect, it } from "vitest";

import type { FsClient } from "./fs";
import type { HttpClient } from "./http";
import type { Repo } from "./repo";
import { fetchReview, originUrl, reviewBaseRef, reviewRef } from "./review";
import { treeAt } from "./revisions";

interface Info {
  github: number;
  gitlab: number;
  token: string;
  base: string;
  head: string;
}

let child: ChildProcess;
let info: Info;
const dirs: string[] = [];

beforeAll(async () => {
  const script = new URL("../../../../testdata/review/fake-host.mjs", import.meta.url).pathname;
  child = spawn("node", [script, "--github-port", "0", "--gitlab-port", "0"], { stdio: ["pipe", "pipe", "inherit"] });
  const line = await new Promise<string>((resolve, reject) => {
    const rl = createInterface({ input: child.stdout! });
    rl.once("line", resolve);
    child.once("error", reject);
    child.once("exit", () => reject(new Error("the fake host exited")));
  });
  info = JSON.parse(line.replace(/^READY /, "")) as Info;
});

afterAll(() => {
  child?.stdin?.end();
  for (const d of dirs.splice(0)) nodeFs.rmSync(d, { recursive: true, force: true });
});

async function cloneRepo(port: number): Promise<Repo> {
  const dir = nodeFs.mkdtempSync(join(tmpdir(), "rt-review-"));
  dirs.push(dir);
  const repo: Repo = { fs: nodeFs as unknown as FsClient, dir, cache: {} };
  await git.clone({ fs: nodeFs, http: nodeHttp, dir, url: `http://127.0.0.1:${port}/acme/orders.git`, singleBranch: true });
  return repo;
}

/** The node client, recording which Authorization headers it sent. */
function spying(): { http: HttpClient; auth: string[] } {
  const auth: string[] = [];
  const http: HttpClient = {
    request: (req) => {
      const header = Object.entries(req.headers ?? {}).find(([k]) => k.toLowerCase() === "authorization");
      if (header) auth.push(header[1]);
      return nodeHttp.request(req as never) as never;
    },
  };
  return { http, auth };
}

describe("fetchReview", () => {
  it("fetches the head and the target into review refs, without a checkout", async () => {
    for (const [kind, port, remoteHeadRef] of [
      ["github", info.github, "refs/pull/1/head"],
      ["gitlab", info.gitlab, "refs/merge-requests/1/head"],
    ] as const) {
      const repo = await cloneRepo(port);
      expect(await originUrl(repo)).toBe(`http://127.0.0.1:${port}/acme/orders.git`);
      const { http } = spying();
      const got = await fetchReview(repo, http, {
        kind,
        number: 1,
        remoteHeadRef,
        target: "main",
        host: `127.0.0.1:${port}`,
        token: info.token,
      });
      expect(got.head_ref).toBe(reviewRef(kind, 1));
      expect(got.head).toBe(info.head);
      expect(got.merge_base).toBe(info.base);
      expect(await git.resolveRef({ fs: nodeFs, dir: repo.dir, ref: reviewRef(kind, 1) })).toBe(info.head);
      expect(await git.resolveRef({ fs: nodeFs, dir: repo.dir, ref: reviewBaseRef(kind, 1) })).toBe(info.base);
      // No checkout: the working folder shows the default branch.
      expect(nodeFs.existsSync(join(repo.dir, "orders/cancel-order.yml"))).toBe(false);
      // The files are readable at the head and at the merge base.
      const atHead = await treeAt(repo, got.head_ref);
      const atBase = await treeAt(repo, got.merge_base);
      expect(Object.keys(atHead.files)).toContain("orders/cancel-order.yml");
      expect(Object.keys(atBase.files)).not.toContain("orders/cancel-order.yml");
      expect(atHead.files["orders/list-orders.yml"]).toContain("limit=25");
    }
  });

  it("sends the token to the review host only, and a missing pull request is an error", async () => {
    const repo = await cloneRepo(info.github);
    const { http, auth } = spying();
    // Another host than the remote's: the token must not go anywhere.
    await expect(
      fetchReview(repo, http, {
        kind: "github",
        number: 2,
        remoteHeadRef: "refs/pull/2/head",
        target: "main",
        host: "github.com",
        token: "ghp_must-not-leave",
      }),
    ).resolves.toMatchObject({ head_ref: reviewRef("github", 2) });
    expect(auth.join("\n")).not.toContain("ghp_must-not-leave");

    await expect(
      fetchReview(repo, http, {
        kind: "github",
        number: 9,
        remoteHeadRef: "refs/pull/9/head",
        target: "main",
        host: `127.0.0.1:${info.github}`,
        token: info.token,
      }),
    ).rejects.toThrow(/refs\/pull\/9\/head|Could not find/);
  });
});
