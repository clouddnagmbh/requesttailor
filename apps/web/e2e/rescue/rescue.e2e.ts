// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 12, strand G: secret rescue in the real web backend. A repository with a token in two
// commits is made with isomorphic-git in a temporary folder (the app's own commit refuses
// credentials), copied into an OPFS folder and opened; the wizard scans it with rt-wasm,
// rewrites it on isomorphic-git after the confirmation, keeps the value in the encrypted
// browser secret store and a new scan is clean. Run: see playwright.real.config.ts.

import * as nodeFs from "node:fs";
import { tmpdir } from "node:os";
import { join, relative } from "node:path";

import { expect, test, type Page } from "@playwright/test";
import git from "isomorphic-git";

import type { Backend } from "@requesttailor/ui";

import { fakePicker } from "../shells/env";

type W = Window & { __rt: { backend: Backend } };

const MOD = "ControlOrMeta";
// Assembled at run time, so secret scanners of the repository stay quiet.
const TOKEN = ["glpat-", "R3scue", "Web", "e2e0123456789"].join("");

const request = (url: string) =>
  `info:\n  name: Get user\n  type: http\nhttp:\n  method: GET\n  url: \"${url}\"\n  headers:\n    - name: PRIVATE-TOKEN\n      value: ${TOKEN}\n`;

/** A repository with the token in both commits of `main`, as files `[path, base64]`. */
async function repository(): Promise<[string, string][]> {
  const dir = nodeFs.mkdtempSync(join(tmpdir(), "rt-rescue-e2e-"));
  try {
    const write = (p: string, t: string) => {
      nodeFs.mkdirSync(join(dir, p, ".."), { recursive: true });
      nodeFs.writeFileSync(join(dir, p), t);
    };
    const author = { name: "Ada", email: "ada@example.com" };
    await git.init({ fs: nodeFs, dir, defaultBranch: "main" });
    write("opencollection.yml", 'opencollection: "1.0.0"\ninfo:\n  name: Rescue test\n');
    write("environments/Local.yml", "name: Local\nvariables:\n  - name: baseUrl\n    value: https://api.example.com\n");
    write("get-user.yml", request("{{baseUrl}}/users/1"));
    for (const f of ["opencollection.yml", "environments/Local.yml", "get-user.yml"]) await git.add({ fs: nodeFs, dir, filepath: f });
    await git.commit({ fs: nodeFs, dir, message: "Add the user request", author });
    write("get-user.yml", request("{{baseUrl}}/users/2"));
    await git.add({ fs: nodeFs, dir, filepath: "get-user.yml" });
    await git.commit({ fs: nodeFs, dir, message: "Second user", author });
    const out: [string, string][] = [];
    const walk = (d: string) => {
      for (const e of nodeFs.readdirSync(d, { withFileTypes: true })) {
        const p = join(d, e.name);
        if (e.isDirectory()) walk(p);
        else out.push([relative(dir, p).split("\\").join("/"), nodeFs.readFileSync(p).toString("base64")]);
      }
    };
    walk(dir);
    return out;
  } finally {
    nodeFs.rmSync(dir, { recursive: true, force: true });
  }
}

async function copyToOpfs(page: Page, files: [string, string][]): Promise<void> {
  await page.evaluate(async (files) => {
    const root = await (await navigator.storage.getDirectory()).getDirectoryHandle("collection", { create: true });
    for (const [path, b64] of files) {
      const parts = path.split("/");
      let dir = root;
      for (const p of parts.slice(0, -1)) dir = await dir.getDirectoryHandle(p, { create: true });
      const w = await (await dir.getFileHandle(parts[parts.length - 1], { create: true })).createWritable();
      await w.write(Uint8Array.from(atob(b64), (c) => c.charCodeAt(0)));
      await w.close();
    }
  }, files);
}

test("rescues a token from the history of an OPFS repository", async ({ page }) => {
  const files = await repository();
  await fakePicker(page);
  await page.goto("/");
  await copyToOpfs(page, files);
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Rescue test");

  await page.keyboard.press(`${MOD}+k`);
  const dialog = page.getByRole("dialog", { name: "Command palette" });
  await dialog.getByRole("combobox").fill("secrets");
  await dialog.getByRole("option").filter({ hasText: "Scan history for secrets" }).click();
  await page.getByTestId("rescue-scan").click();
  const finding = page.getByTestId("rescue-finding");
  await expect(finding).toHaveCount(1);
  await expect(finding).toContainText("get-user.yml:9");
  await expect(finding).toContainText("in 2 commits");
  await expect(finding).toContainText("main");

  await finding.getByTestId("rescue-choose").click();
  await page.getByTestId("rescue-variable").fill("gitlabToken");
  await expect(page.getByTestId("rescue-environment")).toHaveValue("Local");
  await expect(page.getByTestId("rescue-rewrite")).toBeDisabled();
  await page.getByTestId("rescue-confirm").check();
  await page.getByTestId("rescue-rewrite").click();
  await expect(page.getByTestId("rescue-done")).toContainText("Rewrote 2 commits on main");
  await expect(page.getByTestId("rescue-rotate")).toBeVisible();
  // No upstream: no force push needed.
  await expect(page.getByTestId("rescue-force-push")).toHaveCount(0);
  await expect(page.getByTestId("rescue-declared")).toContainText("gitlabToken");

  const state = await page.evaluate(async () => {
    const b = (window as unknown as W).__rt.backend;
    return {
      stored: await b.secrets.get("Local", "gitlabToken"),
      header: (await b.getRequest("get-user.yml")).headers.find((h) => h.name === "PRIVATE-TOKEN")?.value,
      status: await b.git.status(),
      log: await b.git.log(5),
    };
  });
  expect(state.stored).toBe(TOKEN);
  expect(state.header).toBe("{{gitlabToken}}");
  // The rewritten files are checked out: nothing to commit but the declared variable (unsaved).
  expect(state.status.files).toEqual([]);
  expect(state.log.map((c) => c.summary)).toEqual(["Second user", "Add the user request"]);
  expect(await page.content()).not.toContain(TOKEN);

  await page.getByTestId("rescue-rescan").click();
  await expect(page.getByTestId("rescue-clean")).toContainText("No credentials found in 2 commits");
});
