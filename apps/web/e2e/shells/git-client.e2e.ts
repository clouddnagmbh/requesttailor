// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15 (gate items 2 to 8 and 11) with the real web core (rt-wasm, isomorphic-git on OPFS
// folders), one synthetic repository, no network: a folder in Git without a collection opens as
// a repository root; the Changes view shows the changed words and a changed image; history
// search, blame and "Lost work" with Undo; amend, stopping a merge, and a text conflict
// resolved in the 3-way view; and the free UI never shows a staging view (the commit form
// commits the checked files, as since wave 4).

import { expect, test, type Page } from "@playwright/test";

import type { Backend } from "@requesttailor/ui";

import { fillFolder } from "./env";

type W = Window & { __rt: { backend: Backend }; __next: string };
const MOD = "ControlOrMeta";
const manifest = 'opencollection: "1.0.0"\ninfo:\n  name: Work API\n';

const git = <T>(page: Page, run: (g: Backend["git"], b: Backend) => Promise<T>): Promise<T> =>
  page.evaluate(`(${run.toString()})(window.__rt.backend.git, window.__rt.backend)`) as Promise<T>;

async function palette(page: Page, label: string) {
  await page.keyboard.press(`${MOD}+k`);
  const box = page.getByRole("dialog", { name: "Command palette" });
  await box.getByRole("combobox").fill(label);
  await box.getByRole("option").filter({ hasText: label }).first().click();
}

/** Writes a text file, or (with `png`) a PNG of one colour, into the OPFS folder `proj`. */
async function put(page: Page, path: string, text: string | { png: string }) {
  await page.evaluate(
    async ({ path, text }) => {
      let dir = await (await navigator.storage.getDirectory()).getDirectoryHandle("proj", { create: true });
      const parts = path.split("/");
      for (const p of parts.slice(0, -1)) dir = await dir.getDirectoryHandle(p, { create: true });
      let data: string | Blob;
      if (typeof text === "string") data = text;
      else {
        const c = document.createElement("canvas");
        c.width = c.height = 24;
        const g = c.getContext("2d")!;
        g.fillStyle = text.png;
        g.fillRect(0, 0, 24, 24);
        data = await new Promise<Blob>((ok) => c.toBlob((b) => ok(b!), "image/png"));
      }
      const w = await (await dir.getFileHandle(parts[parts.length - 1], { create: true })).createWritable();
      await w.write(data);
      await w.close();
    },
    { path, text },
  );
}

const commit = (page: Page, message: string, paths: string[]) =>
  page.evaluate(({ message, paths }) => (window as unknown as W).__rt.backend.git.commit(message, paths), { message, paths });

const apply = (page: Page, action: unknown) =>
  page.evaluate((a) => (window as unknown as W).__rt.backend.git.apply(a as never), action);

async function confirm(page: Page, options: { done?: boolean; again?: boolean } = {}) {
  const wizard = page.getByTestId("git-wizard");
  await expect(wizard.getByTestId("git-wizard-plan")).toBeVisible();
  await expect(page.getByTestId("git-wizard-confirm")).toBeEnabled();
  await page.getByTestId("git-wizard-confirm").click();
  if (options.again) await page.getByTestId("git-wizard-confirm-again").click();
  await expect(wizard.getByTestId("git-wizard-result")).toBeVisible();
  if (options.done !== false) {
    await page.getByTestId("git-wizard-done").click();
    await expect(wizard).toBeHidden();
  }
}

/** A collection folder, then `git init` in the folder `proj` (a repository root: no collection)
 *  with one commit; the page ends with that repository chosen in the sidebar. */
async function setup(page: Page) {
  await page.addInitScript(() => {
    (window as unknown as W).__next = "work";
    Object.defineProperty(window, "showDirectoryPicker", {
      configurable: true,
      value: async () => (await navigator.storage.getDirectory()).getDirectoryHandle((window as unknown as W).__next, { create: true }),
    });
  });
  await page.goto("/");
  await fillFolder(page, { "opencollection.yml": manifest }, "work");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Work API");

  await page.evaluate(() => ((window as unknown as W).__next = "proj"));
  const entry = await page.evaluate(() => (window as unknown as W).__rt.backend.repositories.init());
  expect(entry?.collections).toEqual([]);
  await git(page, (g) => g.setIdentity({ name: "Ada Lovelace", email: "ada@example.com" }));
  await put(page, "README.md", "# Proj\n\nSee the docs folder.\n");
  await put(page, "logo.png", { png: "#2255cc" });
  await put(page, "notes.txt", "one\ntwo\nthree\n");
  await commit(page, "Add readme, logo and notes", ["/README.md", "/logo.png", "/notes.txt"]);

  await page.reload();
  await page.getByTestId("repository").filter({ hasText: "proj" }).click();
  await expect(page.getByTestId("repository").filter({ hasText: "proj" })).toHaveAttribute("aria-current", "true");
}

test("a repository root: Changes with a word diff and an image diff, commit without staging", async ({ page }) => {
  await setup(page);
  // Opened as a repository: no collection came with it.
  await expect(page.getByTestId("tree-collection")).toHaveCount(1);

  await put(page, "README.md", "# Proj\n\nSee the guides folder.\n");
  await put(page, "logo.png", { png: "#cc5522" });
  await page.getByTestId("view-git").click();
  await expect(page.getByTestId("git-panel")).toBeVisible();
  await expect(page.getByTestId("git-repo-name")).toHaveText("proj");
  await page.getByTestId("git-all-changes").click();
  await expect(page.getByTestId("changes-view")).toBeVisible();
  const rows = page.getByTestId("changes-row");
  await expect(rows).toHaveCount(2);

  await rows.filter({ hasText: "README.md" }).click();
  const removed = page.locator('[data-testid="text-line"][data-kind="removed"]');
  const added = page.locator('[data-testid="text-line"][data-kind="added"]');
  await expect(removed.locator("mark").first()).toHaveText("docs");
  await expect(added.locator("mark").first()).toContainText("guides");

  await rows.filter({ hasText: "logo.png" }).click();
  await expect(page.getByTestId("image-diff")).toBeVisible();
  await expect(page.getByTestId("image-before").locator("img")).toHaveJSProperty("naturalWidth", 24);
  await expect(page.getByTestId("image-after").locator("img")).toHaveJSProperty("naturalWidth", 24);

  // The free UI has no staging: the form lists the files with a check box each and commits them.
  // (The Changes view has a read-only "Staged" filter chip; it stages nothing.)
  await expect(page.getByTestId("git-staging")).toHaveCount(0);
  const staging = await page.locator("[data-testid*=stage]").evaluateAll((e) => e.map((x) => x.getAttribute("data-testid")));
  expect(staging.filter((id) => id !== "changes-staged")).toEqual([]);
  await expect(page.getByRole("button", { name: /^(Stage|Unstage)( |$)/ })).toHaveCount(0);
  await expect(page.getByTestId("git-file")).toHaveCount(2);
  await page.getByTestId("git-message").fill("Guides and a new logo");
  await page.getByTestId("git-commit").click();
  await expect(page.getByTestId("git-clean")).toBeVisible();
  const log = await git(page, (g) => g.log(1));
  expect(log[0].summary).toBe("Guides and a new logo");
});

test("history: search by message and content, blame, lost work comes back and Undo takes it away", async ({ page }) => {
  await setup(page);
  await put(page, "notes.txt", "one\n2\nthree\n");
  await commit(page, "Number the second note", ["/notes.txt"]);

  await palette(page, "Git: Search history");
  await expect(page.getByTestId("git-view")).toHaveAttribute("data-kind", "search");
  await page.getByTestId("search-message").fill("second note");
  await page.getByTestId("search-run").click();
  await expect(page.getByTestId("search-hit")).toHaveCount(1);
  await page.getByTestId("search-message").fill("");
  await page.getByTestId("search-content").fill("three");
  await page.getByTestId("search-run").click();
  await expect(page.getByTestId("search-hit").first()).toContainText("Content");

  // Blame of a file, from the graph's file list.
  await palette(page, "Git: History");
  await expect(page.getByTestId("graph-row").first()).toBeVisible();
  await page.getByTestId("graph-row").filter({ hasText: "Number the second note" }).click();
  await page.getByTestId("graph-file").filter({ hasText: "notes.txt" }).first().click();
  await page.getByTestId("graph-file-history").click();
  await expect(page.getByTestId("git-view")).toHaveAttribute("data-kind", "file-history");
  await expect(page.getByTestId("file-history-entry")).toHaveCount(2);
  await page.getByTestId("file-history-blame").click();
  await expect(page.getByTestId("git-view")).toHaveAttribute("data-kind", "blame");
  await expect(page.getByTestId("blame-line")).toHaveCount(3);
  await expect(page.getByTestId("blame-commit").nth(1)).toContainText("Number the second note");

  // Work that is only on a deleted branch is in the reflog.
  await apply(page, { action: "create-branch", name: "scratch", start: "HEAD", switch: true });
  await put(page, "scratch.txt", "an idea\n");
  await commit(page, "An idea worth keeping", ["/scratch.txt"]);
  await apply(page, { action: "switch", target: "main" });
  const tips = await git(page, async (g) => {
    const refs = await g.refs();
    return { idea: refs.find((r) => r.name === "scratch")!.target, main: refs.find((r) => r.name === "main")!.target };
  });
  await apply(page, { action: "delete-branch", name: "scratch", force: true });
  // isomorphic-git writes no reflog; a repository from elsewhere (a folder opened through the
  // File System Access API) has one, so write what Git would have: the idea commit, then back.
  const who = "Ada Lovelace <ada@example.com> 1795000000 +0000";
  await put(
    page,
    ".git/logs/HEAD",
    [
      `${"0".repeat(40)} ${tips.main} ${who}\tcommit (initial): Add readme, logo and notes`,
      `${tips.main} ${tips.idea} ${who}\tcommit: An idea worth keeping`,
      `${tips.idea} ${tips.main} ${who}\tcheckout: moving from scratch to main`,
      "",
    ].join("\n"),
  );
  const names = () => git(page, async (g) => (await g.refs()).map((r) => r.name));
  expect(await names()).not.toContain("scratch");

  await palette(page, "Git: Lost work");
  await expect(page.getByTestId("git-view")).toHaveAttribute("data-kind", "reflog");
  await expect(page.getByTestId("reflog-entry").filter({ hasText: "An idea worth keeping" }).first()).toBeVisible();
  await page.getByTestId("reflog-entry").filter({ hasText: "An idea worth keeping" }).first().getByTestId("reflog-bring-back").click();
  await expect(page.getByTestId("git-wizard-name")).toHaveValue(/^recovered-/);
  await confirm(page, { done: false });
  expect((await names()).some((n) => n.startsWith("recovered-"))).toBe(true);
  await page.getByTestId("git-wizard-undo").click();
  await expect(page.getByTestId("git-wizard").getByTestId("git-wizard-result")).toContainText("Undone");
  await page.getByTestId("git-wizard-done").click();
  expect((await names()).some((n) => n.startsWith("recovered-"))).toBe(false);
});

test("amend with Undo, stop a merge, and resolve a text conflict in the 3-way view", async ({ page }) => {
  await setup(page);
  await put(page, "notes.txt", "one\ntwo\nthree\nfour\n");
  await commit(page, "Add a fourth note", ["/notes.txt"]);

  await palette(page, "Git: Change last commit");
  await page.getByTestId("git-wizard-message").fill("Add a fourth note (reworded)");
  await expect(page.getByTestId("git-wizard-plan")).toContainText("Replaces the last commit");
  await expect(page.getByTestId("git-wizard-backup")).toBeVisible();
  await confirm(page, { done: false });
  expect((await git(page, (g) => g.log(1)))[0].summary).toBe("Add a fourth note (reworded)");
  await page.getByTestId("git-wizard-undo").click();
  await expect(page.getByTestId("git-wizard").getByTestId("git-wizard-result")).toContainText("Undone");
  await page.getByTestId("git-wizard-done").click();
  expect((await git(page, (g) => g.log(1)))[0].summary).toBe("Add a fourth note");

  // Two branches change the same line.
  const conflict = async (round: number) => {
    await apply(page, { action: "create-branch", name: "other", start: "HEAD", switch: true });
    await put(page, "notes.txt", `one\ntwo (theirs ${round})\nthree\nfour\n`);
    await commit(page, "theirs", ["/notes.txt"]);
    await apply(page, { action: "switch", target: "main" });
    await put(page, "notes.txt", `one\ntwo (ours ${round})\nthree\nfour\n`);
    await commit(page, "ours", ["/notes.txt"]);
    await apply(page, { action: "merge", from: "other" });
    await page.getByTestId("view-requests").click();
    await page.getByTestId("view-git").click();
    await expect(page.locator('[data-testid="git-file"][data-path$="notes.txt"]')).toBeVisible();
  };
  await conflict(1);

  await palette(page, "Git: Stop the merge");
  await expect(page.getByTestId("git-wizard-plan")).toContainText("Stops the");
  await confirm(page, { again: true });
  expect((await git(page, (g) => g.status())).files.some((f) => f.state === "conflicted")).toBe(false);
  await apply(page, { action: "delete-branch", name: "other", force: true });

  await conflict(2);
  await page.locator('[data-testid="git-file"][data-path$="notes.txt"]').click();
  await page.getByTestId("git-resolve-by-hand").click();
  const view = page.getByTestId("text-merge");
  await expect(view).toBeVisible();
  const block = view.getByTestId("text-merge-block").first();
  await expect(block.getByTestId("text-merge-mine")).toContainText("two (ours 2)");
  await expect(block.getByTestId("text-merge-their")).toContainText("two (theirs 2)");
  await block.getByTestId("text-merge-theirs").click();
  await expect(view.getByTestId("text-merge-result")).toHaveValue(/two \(theirs 2\)/);
  await view.getByTestId("text-merge-resolve").click();
  await expect(view).toBeHidden();
  expect((await git(page, (g) => g.status())).files.some((f) => f.state === "conflicted")).toBe(false);

  // The next commit concludes the merge.
  await page.getByTestId("view-requests").click();
  await page.getByTestId("view-git").click();
  await page.getByTestId("git-message").fill("Merge other");
  await page.getByTestId("git-commit").click();
  await expect(page.getByTestId("git-clean")).toBeVisible();
  const log = await git(page, (g) => g.log(1));
  expect(log[0].summary).toBe("Merge other");
  expect(await page.evaluate(async () => (await (await (await (await navigator.storage.getDirectory()).getDirectoryHandle("proj")).getFileHandle("notes.txt")).getFile()).text())).toContain("two (theirs 2)");
});
