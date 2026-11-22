// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 5, strand E with the real core in the browser (rt-wasm's scanSecrets) on an OPFS
// folder: a request file with a plaintext token blocks the commit with file, line and hint;
// after replacing it with a variable, the commit goes through. Also: sending with external
// secrets and no companion says how to start it. The fake token is assembled at run time.

import { expect, test, type Page } from "@playwright/test";

import { fakePicker, fillFolder, sampleCollection } from "./env";

type W = Window & { __rt: { backend: import("@requesttailor/ui").Backend } };

const TOKEN = ["9c1e2f3a4b5c", "6d7e8f90a1b2"].join("");

const me = (token: string) =>
  [
    "info:",
    "  name: Me",
    "  type: http",
    "  seq: 3",
    "",
    "http:",
    "  method: GET",
    '  url: "{{baseUrl}}/echo/me"',
    "  auth:",
    "    type: bearer",
    `    token: ${token}`,
    "",
  ].join("\n");

async function open(page: Page, files: Record<string, string>) {
  await fakePicker(page);
  await page.goto("/");
  await fillFolder(page, files);
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Shell test");
}

test("the real scanner blocks a commit with a plaintext token", async ({ page }) => {
  await open(page, { ...sampleCollection(), "me.yml": me(TOKEN) });
  await page.getByTestId("view-git").click();
  await page.getByTestId("git-init").click();
  await page.evaluate(() => (window as unknown as W).__rt.backend.git.setIdentity({ name: "Ada", email: "ada@example.com" }));

  await page.getByTestId("git-message").fill("Initial collection");
  await page.getByTestId("git-commit").click();
  const finding = page.getByTestId("git-secret-finding");
  await expect(finding).toHaveCount(1);
  await expect(finding.getByTestId("git-secret-location")).toHaveText("me.yml:11");
  await expect(finding.getByTestId("git-secret-snippet")).toHaveText("token: ***");
  await expect(finding.getByTestId("git-secret-hint")).toContainText("{{token}}");
  await expect(page.getByTestId("git-secrets")).not.toContainText(TOKEN);
  await expect(page.getByTestId("git-log")).toHaveCount(0);

  // The backend refuses on its own too, with the desktop's message.
  const refused = await page.evaluate(async () => {
    const git = (window as unknown as W).__rt.backend.git;
    try {
      await git.commit("x", ["me.yml"]);
      return null;
    } catch (e) {
      return { kind: (e as { kind?: string }).kind, message: (e as Error).message };
    }
  });
  expect(refused?.kind).toBe("secrets-found");
  expect(refused?.message).toMatch(/^Commit refused: 1 credential in 1 file \(me\.yml:11 token\)/);
  expect(refused?.message).not.toContain(TOKEN);

  // A variable instead: the commit goes through.
  await fillFolder(page, { "me.yml": me('"{{token}}"') });
  expect(await page.evaluate(() => (window as unknown as W).__rt.backend.git.scan(["me.yml"]))).toEqual([]);
  await page.evaluate(() => (window as unknown as W).__rt.backend.git.commit("Initial collection", ["opencollection.yml", "me.yml"]));
  expect(
    (await page.evaluate(() => (window as unknown as W).__rt.backend.git.log(5))).map((c) => c.summary),
  ).toEqual(["Initial collection"]);
});

test("external secrets without a companion: the send says how to start it", async ({ page }) => {
  const files = sampleCollection();
  files["environments/Local.yml"] +=
    "externalSecrets:\n  type: one-password\n  variables:\n    - name: token\n      secretReference: op://Dev/api/token\n";
  await open(page, files);
  // A fresh browser profile has no pairing token, so there is no companion to ask.
  const error = await page.evaluate(async () => {
    const b = (window as unknown as W).__rt.backend;
    try {
      await b.send("health.yml", { environment: "Local", vars: [] });
      return null;
    } catch (e) {
      return { kind: (e as { kind?: string }).kind, message: (e as Error).message };
    }
  });
  expect(error?.kind).toBe("unsupported");
  expect(error?.message).toContain("reads secrets from 1Password");
  expect(error?.message).toContain("rt agent --allow-secrets");
});
