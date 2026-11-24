// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 12, strand E: pull-request review in the real web backend against the fake Git host
// (testdata/review/fake-host.mjs, GitHub on 48041, GitLab on 48042) through a real `rt agent`
// (48044): clone the repository, add the host, give the token, open the PR (its head is
// fetched into a review ref by isomorphic-git, no checkout), read the semantic diff, see the
// API's breaking changes, run the changed requests against rt-testserver, comment on a field,
// approve. Needs RT_BIN (the `rt` binary) and `git`; see playwright.real.config.ts.

import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { expect, test, type Page } from "@playwright/test";

import type { Backend } from "@requesttailor/ui";

import { hasGit, startProcess } from "../git/server";
import { fakePicker, fillFolder } from "../shells/env";
import { SERVER_PORT } from "./playwright.real.config";

const GITHUB_PORT = 48041;
const GITLAB_PORT = 48042;
const AGENT_PORT = 48044;
const { RT_BIN } = process.env;
const TOKEN = "fake-token-1f4c9a";
const MOD = "ControlOrMeta";
const HOST_SCRIPT = join(fileURLToPath(new URL("../../../..", import.meta.url)), "testdata/review/fake-host.mjs");

type W = Window & { __rt: { backend: Backend } };

test.skip(!hasGit() || !RT_BIN, "needs `git` and RT_BIN (the rt binary) to fetch through a real rt agent");
test.describe.configure({ mode: "serial" });

let host: ReturnType<typeof startProcess>;
let agent: ReturnType<typeof startProcess>;

test.beforeAll(async ({}, info) => {
  const origin = new URL(String(info.project.use.baseURL)).origin;
  host = startProcess("node", [HOST_SCRIPT, "--github-port", String(GITHUB_PORT), "--gitlab-port", String(GITLAB_PORT), "--no-stdin-exit"]);
  await host.line(/^READY /);
  agent = startProcess(RT_BIN!, ["agent", "--port", String(AGENT_PORT), "--allow-origin", origin]);
});

test.afterAll(() => {
  agent?.child.kill();
  host?.child.kill();
});

const state = async (page: Page, port: number) => (await page.request.get(`http://127.0.0.1:${port}/__state`)).json();

/** Pairs with the companion and clones the fake host's repository into the OPFS folder. */
async function cloneRepo(page: Page, port: number, paired = true) {
  if (paired) {
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
    await page.goto(`/#rt-pair=${code}`);
  } else {
    await fakePicker(page, "workspace");
    await page.goto("/");
  }
  await fillFolder(page, { "opencollection.yml": 'opencollection: "1.0.0"\ninfo:\n  name: Workspace\n' }, "workspace");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Workspace");
  await page.getByTestId("view-git").click();
  await page.getByTestId("git-clone-open").click();
  await page.getByTestId("git-clone-url").fill(`http://127.0.0.1:${port}/acme/orders.git`);
  await page.getByTestId("git-clone").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Orders", { timeout: 30_000 });
  // The reviewer's own environment: the requests go to rt-testserver.
  await page.evaluate(async (server) => {
    const b = (window as unknown as W).__rt.backend;
    const env = (await b.environments()).find((e) => e.name === "Local")!;
    await b.updateEnvironment({
      ...env,
      variables: env.variables.map((v) => (v.name === "baseUrl" ? { ...v, value: `${server}/echo` } : v)),
    });
  }, `http://127.0.0.1:${SERVER_PORT}`);
}

async function openReview(page: Page) {
  await page.keyboard.press(`${MOD}+k`);
  const dialog = page.getByRole("dialog", { name: "Command palette" });
  await dialog.getByRole("combobox").fill("review");
  await dialog.getByRole("option").filter({ hasText: "Review: pull and merge requests" }).click();
  await expect(page.getByTestId("review-panel")).toBeVisible();
}

async function addHost(page: Page, port: number, kind: "github" | "gitlab") {
  await page.getByTestId("review-host").fill(`127.0.0.1:${port}`);
  await page.getByTestId("review-kind").selectOption(kind);
  await page.getByTestId("review-api").fill(`http://127.0.0.1:${port}/${kind === "github" ? "api/v3" : "api/v4"}`);
  await page.getByTestId("review-add-host").click();
  await page.getByTestId("review-token").fill(TOKEN);
  await page.getByTestId("review-save-token").click();
}

for (const [kind, port] of [
  ["github", GITHUB_PORT],
  ["gitlab", GITLAB_PORT],
] as const) {
  test(`reviews a ${kind === "github" ? "pull" : "merge"} request end to end`, async ({ page }) => {
    await cloneRepo(page, port);
    await openReview(page);
    await expect(page.getByTestId("review-no-host")).toBeVisible();
    await addHost(page, port, kind);
    const rows = page.getByTestId("review-row");
    await expect(rows).toHaveCount(2);
    await expect(rows.first()).toContainText("Add currency and cancel-order");
    await expect(rows.nth(1).getByTestId("review-draft")).toBeVisible();
    // The token is in the encrypted store, not on the page.
    expect(await page.content()).not.toContain(TOKEN);

    // Open: the head is fetched by isomorphic-git through the companion, no checkout.
    await rows.first().click();
    await expect(page.getByTestId("review-title")).toHaveText("#1 Add currency and cancel-order", { timeout: 30_000 });
    await expect(page.getByTestId("review-ci")).toHaveAttribute("data-state", "failure");
    const changes = page.getByTestId("sd-change");
    await expect(changes).toHaveCount(3);
    await expect(changes.filter({ hasText: "orders/cancel-order.yml" })).toHaveAttribute("data-kind", "added");
    await expect(changes.filter({ hasText: "orders/list-orders.yml" })).toHaveAttribute("data-kind", "modified");
    // The OpenAPI spec lost fields between the two revisions.
    await expect(page.getByTestId("contract-notes")).toBeVisible();
    // The working folder was not touched (no checkout).
    expect(await page.evaluate(() => (window as unknown as W).__rt.backend.tree().then((t) => JSON.stringify(t)))).not.toContain("cancel-order");

    // Run the three changed requests at the head against rt-testserver.
    await page.getByTestId("review-run").click();
    await expect(page.getByTestId("review-step")).toHaveCount(3, { timeout: 30_000 });
    await expect(page.getByTestId("review-step-status").first()).toHaveText("200");
    await expect(page.getByTestId("review-run-summary")).toContainText("3 of 3 passed");

    // The host's own comments: one on a line, one general.
    await expect(page.getByTestId("review-comment-group").filter({ hasText: "Is `currency` required?" })).toBeVisible();
    await expect(page.getByTestId("review-comment").filter({ hasText: "Old remark" })).toHaveAttribute("data-outdated", "true");

    // Comment on a changed field of list-orders.yml (the line is the field's own line).
    await changes.filter({ hasText: "orders/list-orders.yml" }).getByTestId("sd-comment").first().click();
    await page.getByTestId("review-comment-text").fill("Why 25?");
    await page.getByTestId("review-comment-send").click();
    await expect(page.getByTestId("review-comment").filter({ hasText: "Why 25?" })).toBeVisible();
    await expect(page.getByTestId("review-comment").filter({ hasText: "Why 25?" }).getByTestId("review-comment-field")).toBeVisible();

    await page.getByTestId("review-body").fill("Looks good to me");
    await page.getByTestId("review-approve").click();
    await expect(page.getByTestId("review-sent")).toHaveText("Approved.");

    // What the host received.
    const s = await state(page, port);
    expect(s.comments.some((c: { body: string; path: string }) => c.body === "Why 25?" && c.path === "orders/list-orders.yml")).toBe(true);
    if (kind === "github") expect(s.reviews).toMatchObject([{ number: 1, event: "APPROVE", body: "Looks good to me" }]);
    else expect(s.approvals).toEqual([{ number: 1, approved: true }]);
    // Every API call carried the token as a bearer header; the agent log shows the Git fetch.
    expect(s.authorizations.length).toBeGreaterThan(0);
    expect(agent.stderr()).toMatch(new RegExp(`127\\.0\\.0\\.1:${port} -> 200`));
    expect(await page.content()).not.toContain(TOKEN);
  });
}

// Wave 15, strand E: "Create pull request" against the fake host. The pull request is made from
// a local branch that is on the server (`feature/currency`, as cloned) into `chore/tenant`: the
// host already has one for `main`, so this pair is free, and the same pair again is refused.
for (const [kind, port] of [
  ["github", GITHUB_PORT],
  ["gitlab", GITLAB_PORT],
] as const) {
  test(`creates a ${kind === "github" ? "pull" : "merge"} request on the host and says what it refused`, async ({ page }) => {
    // The host's state is shared by both ports and by the tests: start and leave it as new.
    await page.request.post(`http://127.0.0.1:${port}/__reset`);
    await cloneRepo(page, port);
    await page.evaluate(() =>
      (window as unknown as W).__rt.backend.git.apply({ action: "create-branch", name: "feature/currency", start: "origin/feature/currency" }),
    );
    await openReview(page);
    await addHost(page, port, kind);
    await expect(page.getByTestId("review-row")).toHaveCount(2);
    await page.getByTestId("review-new").click();
    await expect(page.getByTestId("new-pull")).toBeVisible();
    await expect(page.getByTestId("new-pull-repo")).toContainText("acme/orders");
    await page.getByTestId("new-pull-head").selectOption("feature/currency");
    await page.getByTestId("new-pull-base").selectOption("chore/tenant");
    await page.getByTestId("new-pull-title").fill("Currency on top of the tenant");
    await page.getByTestId("new-pull-draft").check();
    await page.getByTestId("new-pull-create").click();
    await expect(page.getByTestId("new-pull-done")).toContainText("Currency on top of the tenant", { timeout: 30_000 });

    // What the host received: the right branches, a draft, the user's token.
    const s = await state(page, port);
    expect(s.created).toHaveLength(1);
    const made = s.created[0];
    if (kind === "github") expect(made).toMatchObject({ title: "Currency on top of the tenant", head: "feature/currency", base: "chore/tenant", draft: true });
    else expect(made).toMatchObject({ title: "Draft: Currency on top of the tenant", source_branch: "feature/currency", target_branch: "chore/tenant" });
    expect(s.authorizations.length).toBeGreaterThan(0);
    expect(await page.content()).not.toContain(TOKEN);

    await page.getByTestId("new-pull-close").click();
    await expect(page.getByTestId("review-row")).toHaveCount(3);
    await expect(page.getByTestId("review-row").filter({ hasText: "Currency on top of the tenant" }).getByTestId("review-draft")).toBeVisible();

    // The same pair again: the host's own words.
    await page.getByTestId("review-new").click();
    await page.getByTestId("new-pull-head").selectOption("feature/currency");
    await page.getByTestId("new-pull-base").selectOption("chore/tenant");
    await page.getByTestId("new-pull-title").fill("Again");
    await page.getByTestId("new-pull-create").click();
    await expect(page.getByTestId("new-pull-error")).toContainText(/already/i);
    // The host saw the second attempt (it records every body) and made no third pull request.
    expect((await state(page, port)).created).toHaveLength(2);
    await page.getByTestId("new-pull-cancel").click();
    await expect(page.getByTestId("review-row")).toHaveCount(3);
    await page.request.post(`http://127.0.0.1:${port}/__reset`);
  });
}

test("without the companion the list works (fetch) and opening explains what is needed", async ({ page }) => {
  await cloneRepoWithoutCompanion(page);
  await openReview(page);
  await addHost(page, GITHUB_PORT, "github");
  await expect(page.getByTestId("review-row")).toHaveCount(2);
  await page.getByTestId("review-row").first().click();
  await expect(page.getByTestId("review-pull-error")).toContainText("rt agent");
});

/** A local repository whose `origin` is the fake host, made without the companion: files and
 *  `.git/config` written straight into the OPFS folder, no history needed to list. */
async function cloneRepoWithoutCompanion(page: Page) {
  await fakePicker(page, "workspace");
  await page.goto("/");
  await fillFolder(page, {
    "opencollection.yml": 'opencollection: "1.0.0"\ninfo:\n  name: Offline\n',
    ".git/HEAD": "ref: refs/heads/main\n",
    ".git/config": `[core]\n\trepositoryformatversion = 0\n[remote "origin"]\n\turl = http://127.0.0.1:${GITHUB_PORT}/acme/orders.git\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n`,
  }, "workspace");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Offline");
}
