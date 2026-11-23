// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 4 gate, strand A2: Git in the web app with the real core and isomorphic-git on an
// OPFS folder (fake folder picker from env.ts).
//   1. init → commit → new branch → change → commit → switch: the files on disk follow.
//   2. clone → push → pull through a real `rt agent` to a local smart-HTTP Git server
//      (`git http-backend`) behind Basic auth: the credentials prompt, the push and a
//      colleague's commit arriving by pull. Needs `git` and RT_BIN (the `rt` binary), e.g.
//        cargo build -p rt-cli
//        RT_BIN=target/debug/rt npm run e2e:shells -w apps/web
//      and is skipped with a message otherwise.
// The page's companion client talks to the default port 47812; an init script points those
// requests at this test's agent (47830), so a companion the user runs is never touched.

import { join } from "node:path";
import { readFileSync, writeFileSync } from "node:fs";

import { expect, test, type Page } from "@playwright/test";

import { branchMenu, newBranch, syncMenu } from "../git-wizards/helpers";

import { gitCli, hasGit, startGitServer, startProcess, type GitServer } from "../git/server";
import { fakePicker, fillFolder, readFolderFile, sampleCollection } from "./env";

// Not 47830/47831: rt-odata-testserver listens there (wave 6).
const AGENT_PORT = 47824;
const GIT_PORT = 47825;
const { RT_BIN } = process.env;

type W = Window & { __rt: { backend: import("@requesttailor/ui").Backend } };

async function openFolder(page: Page, name: string) {
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText(name);
}

async function setIdentityWhenAsked(page: Page) {
  const form = page.getByTestId("git-identity");
  await expect(form).toBeVisible();
  await form.getByTestId("git-identity-name").fill("Ada Lovelace");
  await form.getByTestId("git-identity-email").fill("ada@example.com");
  await form.getByTestId("git-identity-save").click();
  await expect(form).toBeHidden();
}

async function changeMethod(page: Page, request: string, method: string) {
  await page.getByTestId("view-requests").click();
  await page.getByTestId("tree-request").filter({ hasText: request }).click();
  await page.getByTestId("method-select").selectOption(method);
  await page.getByTestId("save").click();
  await expect(page.getByTestId("save")).toBeDisabled();
  await page.getByTestId("view-git").click();
}

const file = (page: Page, path: string) => page.locator(`[data-testid="git-file"][data-path="${path}"]`);

test("init, commit, create a branch, switch: the files follow the branch", async ({ page }) => {
  await fakePicker(page);
  await page.goto("/");
  // The sample has a stray `.git/config`; it is not a repository until "Initialize".
  await fillFolder(page, sampleCollection());
  await openFolder(page, "Shell test");
  await page.getByTestId("view-git").click();
  await expect(page.getByTestId("git-norepo")).toContainText("collection folder itself");
  await page.getByTestId("git-init").click();
  await expect(page.getByTestId("git-branch")).toHaveText("main (no commits yet)");
  await expect(file(page, "health.yml").getByTestId("git-file-state")).toHaveText("A");

  await page.getByTestId("git-message").fill("Initial collection");
  await page.getByTestId("git-commit").click();
  await setIdentityWhenAsked(page);
  await expect(page.getByTestId("git-clean")).toBeVisible();
  await expect(page.getByTestId("git-branch")).toHaveText("main");

  await newBranch(page, "feature");
  await expect(page.getByTestId("git-branch")).toHaveText("feature");

  await changeMethod(page, "Health", "POST");
  await expect(file(page, "health.yml").getByTestId("git-file-state")).toHaveText("M");
  await file(page, "health.yml").getByTestId("git-file-open").click();
  // The diff comes from the core (rt-wasm textDiff), the same as on the desktop.
  const diff = page.getByTestId("git-diff");
  await expect(diff.locator('[data-kind="removed"]')).toHaveText(["-  method: GET"]);
  await expect(diff.locator('[data-kind="added"]')).toHaveText(["+  method: POST"]);
  await diff.getByTestId("git-diff-close").click();
  await page.getByTestId("git-message").fill("Health is a POST");
  await page.getByTestId("git-commit").click();
  await expect(page.getByTestId("git-clean")).toBeVisible();
  await expect(page.getByTestId("git-commit-subject")).toHaveText(["Health is a POST", "Initial collection"]);

  await branchMenu(page, "main");
  await expect(page.getByTestId("git-branch")).toHaveText("main");
  expect(await readFolderFile(page, "health.yml")).toContain("method: GET");
  await expect(page.getByTestId("method-select")).toHaveValue("GET");
  expect((await page.evaluate(() => (window as unknown as W).__rt.backend.getRequest("health.yml"))).method).toBe("GET");

  await branchMenu(page, "feature");
  await expect(page.getByTestId("git-branch")).toHaveText("feature");
  expect(await readFolderFile(page, "health.yml")).toContain("method: POST");
  await expect(page.getByTestId("method-select")).toHaveValue("POST");
});

test.describe("through a real rt agent", () => {
  test.skip(!hasGit() || !RT_BIN, "needs `git` and RT_BIN (the rt binary) to push and pull through a real rt agent");
  test.describe.configure({ mode: "serial" });

  let server: GitServer;
  let agent: ReturnType<typeof startProcess>;
  let repoUrl: string;

  test.beforeAll(async ({}, info) => {
    const origin = new URL(String(info.project.use.baseURL)).origin;
    server = await startGitServer(GIT_PORT, { username: "ada", password: "s3cret-token" });
    const files = sampleCollection();
    delete files[".git/config"];
    delete files["node_modules/pkg/x.yml"];
    delete files[".DS_Store"];
    repoUrl = server.repository("demo", files);
    agent = startProcess(RT_BIN!, ["agent", "--port", String(AGENT_PORT), "--allow-origin", origin]);
  });

  test.afterAll(async () => {
    agent?.child.kill();
    await server?.stop();
  });

  test("clone, push with credentials, pull a colleague's commit", async ({ page }) => {
    const code = (await agent.line(/#rt-pair=(\S+)/))[1];
    await page.addInitScript(
      ({ from, to }) => {
        const original = window.fetch.bind(window);
        window.fetch = (input: RequestInfo | URL, init?: RequestInit) =>
          original(typeof input === "string" && input.startsWith(from) ? to + input.slice(from.length) : input, init);
      },
      { from: "http://127.0.0.1:47812", to: `http://127.0.0.1:${AGENT_PORT}` },
    );
    await fakePicker(page, "workspace");
    // The pairing link pairs this page with the agent as it opens.
    await page.goto(`/#rt-pair=${code}`);
    await fillFolder(page, { "opencollection.yml": 'opencollection: "1.0.0"\ninfo:\n  name: Workspace\n' }, "workspace");
    await openFolder(page, "Workspace");
    await page.getByTestId("view-git").click();

    // Clone: the server wants credentials, the prompt stores them and the clone runs again.
    await page.getByTestId("git-clone-url").fill(repoUrl);
    await page.getByTestId("git-clone").click();
    const prompt = page.getByTestId("git-credentials");
    await expect(prompt).toContainText(`127.0.0.1:${GIT_PORT}`);
    await prompt.getByTestId("git-cred-username").fill("ada");
    await prompt.getByTestId("git-cred-password").fill("s3cret-token");
    await prompt.getByTestId("git-cred-save").click();
    await expect(page.getByTestId("collection-name")).toHaveText("Shell test", { timeout: 20_000 });
    await expect(page.getByTestId("git-branch")).toHaveText("main");
    await expect(page.getByTestId("git-sync")).toHaveText("Up to date");
    expect(await readFolderFile(page, "demo/health.yml", "workspace")).toContain("method: GET");

    // Commit and push.
    await changeMethod(page, "Health", "PUT");
    await page.getByTestId("git-message").fill("Health is a PUT");
    await page.getByTestId("git-commit").click();
    await setIdentityWhenAsked(page);
    await expect(page.getByTestId("git-sync")).toHaveAttribute("data-ahead", "1");
    await syncMenu(page, /^Push/);
    await expect(page.getByTestId("git-result")).toHaveText("Pushed main to origin/main.", { timeout: 20_000 });
    await expect(page.getByTestId("git-sync")).toHaveText("Up to date");
    const bare = join(server.root, "demo.git");
    expect(gitCli(bare, "log", "-1", "--format=%s|%an <%ae>").trim()).toBe("Health is a PUT|Ada Lovelace <ada@example.com>");
    expect(gitCli(bare, "show", "main:health.yml")).toContain("method: PUT");

    // A colleague pushes; pull brings the change into the open collection.
    const colleague = server.workingClone("demo");
    writeFileSync(
      join(colleague, "colleague.yml"),
      ["info:", "  name: From colleague", "  type: http", "  seq: 3", "", "http:", "  method: GET", '  url: "{{baseUrl}}/echo/colleague"', ""].join("\n"),
    );
    const env = readFileSync(join(colleague, "environments/Local.yml"), "utf8");
    writeFileSync(join(colleague, "environments/Local.yml"), env.replace("value: http", "value: https"));
    gitCli(colleague, "add", "-A");
    gitCli(colleague, "commit", "-q", "-m", "Add a request");
    gitCli(colleague, "push", "-q", "origin", "main");

    await syncMenu(page, /^Pull/);
    await expect(page.getByTestId("git-result")).toHaveText("Pulled 1 commit (fast-forward).", { timeout: 20_000 });
    expect(await readFolderFile(page, "demo/colleague.yml", "workspace")).toContain("From colleague");
    await page.getByTestId("view-requests").click();
    await expect(page.getByTestId("tree-request").filter({ hasText: "From colleague" })).toBeVisible();
    // Through the companion, natively: the agent logged each Git request.
    expect(agent.stderr()).toMatch(new RegExp(`POST 127\\.0\\.0\\.1:${GIT_PORT} -> 200`));
  });
});
