// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Acceptance tests of wave 8, strand C (agents), against the mock backend: the approval
// banner (visible on every screen, approve and deny with the keyboard and the mouse), the
// policy editor per environment and the audit log.

import { expect, test, type Page } from "@playwright/test";

import type { AgentPolicy, MockBackend } from "@requesttailor/ui";

const MOD = "ControlOrMeta";

/** `window.__rt` of the mock shell; `agents` is the scripted `MockAgents`. */
type W = {
  __rt: {
    backend: MockBackend & {
      agents: MockBackend["agents"] & { request(p: object): { id: string }; decisions: { id: string; decision: string }[] };
    };
  };
};

const decisions = (page: Page) =>
  page.evaluate(() => (window as unknown as W).__rt.backend.agents.decisions) as Promise<
    { id: string; decision: string }[]
  >;

const policy = (page: Page) =>
  page.evaluate(() => (window as unknown as W).__rt.backend.agents.policy()) as Promise<AgentPolicy>;

async function palette(page: Page, query: string, option: string) {
  await page.keyboard.press(`${MOD}+k`);
  const dialog = page.getByRole("dialog", { name: "Command palette" });
  await dialog.getByRole("combobox").fill(query);
  await dialog.getByRole("option").filter({ hasText: option }).click();
}

test("a waiting approval shows on every screen and is approved from the keyboard", async ({ page }) => {
  await page.goto("/?backend=mock&agents=1");
  // Before a collection is open: approvals are machine-wide.
  const banner = page.getByTestId("agent-approval");
  await expect(banner).toHaveCount(1);
  await expect(banner.getByTestId("agent-approval-method")).toHaveText("POST");
  await expect(banner.getByTestId("agent-approval-url")).toHaveText("https://api.example.com/users?api_key=****");
  await expect(banner.getByTestId("agent-approval-environment")).toHaveText("PRD");
  await expect(banner.getByTestId("agent-approval-agent")).toHaveText("claude-code");

  // With a request open, the banner is still there and takes no focus.
  await page.getByTestId("open-collection").click();
  await page.getByTestId("tree-request").first().click();
  await expect(page.getByTestId("request-editor")).toBeVisible();
  await expect(banner).toHaveCount(1);
  await expect(page.getByTestId("agent-approve")).not.toBeFocused();

  await page.getByTestId("agent-approve").focus();
  await page.keyboard.press("Enter");
  await expect(banner).toHaveCount(0);
  expect(await decisions(page)).toEqual([{ id: "0123456789abcdef0123456789abcdef", decision: "approved" }]);

  // The decision is in the audit log.
  await palette(page, "audit", "Agent audit log");
  await expect(page.getByTestId("agent-audit-row")).toHaveCount(1);
  await expect(page.getByTestId("agent-audit-outcome")).toContainText("Sent 201");
  await expect(page.getByTestId("agent-audit-outcome")).toContainText("approved by app");
});

test("deny answers only the chosen approval", async ({ page }) => {
  await page.goto("/?backend=mock");
  await expect(page.getByTestId("agent-approvals")).toHaveCount(0);
  const ids = await page.evaluate(() => {
    const a = (window as unknown as W).__rt.backend.agents;
    return [
      a.request({ method: "DELETE", url: "https://api.example.com/users/7" }).id,
      a.request({ method: "PATCH", url: "https://api.example.com/users/8", agent: "cursor" }).id,
    ];
  });
  const rows = page.getByTestId("agent-approval");
  await expect(rows).toHaveCount(2);
  await rows.filter({ hasText: "DELETE" }).getByTestId("agent-deny").click();
  await expect(rows).toHaveCount(1);
  await expect(rows.getByTestId("agent-approval-agent")).toHaveText("cursor");
  expect(await decisions(page)).toEqual([{ id: ids[0], decision: "denied" }]);

  await page.getByRole("button", { name: "Deny PATCH https://api.example.com/users/8 in PRD" }).press("Space");
  await expect(rows).toHaveCount(0);
  expect((await decisions(page)).map((d) => d.decision)).toEqual(["denied", "denied"]);
});

test("the policy editor sets access and the production flag per environment", async ({ page }) => {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("tree-request").first()).toBeVisible();
  await palette(page, "agent access", "Agent access");

  const editor = page.getByTestId("agent-policy");
  await expect(editor).toBeVisible();
  const row = (name: string) => editor.locator(`[data-testid="agent-policy-row"][data-environment="${name}"]`);
  await expect(row("Local")).toBeVisible();
  await expect(row("PRD").getByTestId("agent-policy-looks-production")).toBeVisible();
  await expect(row("PRD").getByTestId("agent-policy-effective")).toHaveText("Reading requests only");

  await editor.getByTestId("agent-policy-default").selectOption("write");
  await row("PRD").getByTestId("agent-policy-production").check();
  await expect(row("PRD").getByTestId("agent-policy-effective")).toHaveText("Writing requests wait for approval");
  await expect(row("PRD").getByTestId("agent-policy-looks-production")).toHaveCount(0);
  await row("Local").getByTestId("agent-policy-access").selectOption("none");
  await expect(row("Local").getByTestId("agent-policy-effective")).toHaveText("Hidden from agents");

  await expect
    .poll(() => policy(page))
    .toEqual({
      access: "write",
      environments: [
        { environment: "Local", access: "none", production: false },
        { environment: "PRD", access: "write", production: true },
      ],
    });

  // Reopened, the editor shows what the backend holds.
  await page.keyboard.press("Escape");
  await expect(editor).toBeHidden();
  await palette(page, "agent access", "Agent access");
  await expect(row("Local").getByTestId("agent-policy-access")).toHaveValue("none");
  await expect(row("PRD").getByTestId("agent-policy-production")).toBeChecked();
  await expect(editor.getByTestId("agent-policy-default")).toHaveValue("write");

  // Back to the default: no entry is left for Local.
  await row("Local").getByTestId("agent-policy-access").selectOption("default");
  await expect
    .poll(async () => (await policy(page)).environments.map((e) => e.environment))
    .toEqual(["PRD"]);
});
