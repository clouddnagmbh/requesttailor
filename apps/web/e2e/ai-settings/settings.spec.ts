// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 16, strand A against the mock backend: profiles and roles, the "what will be sent"
// preview, the per-collection opt-out, and the off state. Screenshots: RT_AI_SHOTS=<dir>.

import { expect, test, type Page } from "@playwright/test";

import type { MockBackend } from "@requesttailor/ui";

type W = { __rt: { backend: MockBackend }; __demoAi?: { text?: string; error?: string } };
const MOD = process.platform === "darwin" ? "Meta" : "Control";
const SHOTS = process.env.RT_AI_SHOTS;

async function open(page: Page, extra = "") {
  await page.goto(`/?backend=mock&extensions=demo${extra}`);
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("tree-request").first()).toBeVisible();
}

async function command(page: Page, label: string) {
  await page.keyboard.press(`${MOD}+k`);
  await page.getByTestId("palette-input").fill(label);
  await page.getByTestId("palette-option").filter({ hasText: label }).first().click();
}

async function shot(page: Page, name: string) {
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/${name}.png` });
}

async function noOverflow(page: Page, testid: string) {
  const over = await page.getByTestId(testid).evaluate((el) => {
    const dlg = el.closest('[role="dialog"]') ?? el;
    return [...dlg.querySelectorAll<HTMLElement>("*")].filter((e) => e.scrollWidth > e.clientWidth + 1 && getComputedStyle(e).overflowX === "visible" && e.clientWidth > 0).length;
  });
  expect(over).toBe(0);
}

async function addProfile(page: Page, provider: string, model: string, key: string) {
  await page.getByTestId("ai-provider").selectOption(provider);
  await page.getByTestId("ai-model").fill(model);
  if (key) await page.getByTestId("ai-key").fill(key);
}

for (const size of [{ width: 1440, height: 900 }, { width: 900, height: 700 }]) {
  for (const theme of ["light", "dark"]) {
    test(`settings: two profiles with roles, no overflow at ${size.width}px, ${theme}`, async ({ page }) => {
      await page.setViewportSize(size);
      await page.emulateMedia({ colorScheme: theme as "light" | "dark" });
      await open(page);
      await command(page, "AI assistant: settings");
      const dialog = page.getByTestId("ai-settings");
      await expect(dialog).toBeVisible();
      // Every provider is offered, none is chosen.
      const options = await page.getByTestId("ai-provider").locator("option").allTextContents();
      expect(options.length).toBeGreaterThanOrEqual(7);
      await expect(page.getByTestId("ai-provider")).toHaveValue("");
      await shot(page, `settings-empty-${theme}-${size.width}`);
      await addProfile(page, "openai-compatible", "llama3.1", "");
      await page.getByTestId("ai-base-url").fill("http://localhost:11434/v1");
      await expect(page.getByTestId("ai-local-note")).toBeVisible();
      await page.getByTestId("ai-add-profile").click();
      await addProfile(page, "anthropic", "claude-opus-5-5", "sk-ant-secret");
      await expect(page.getByTestId("ai-roles")).toBeVisible();
      await page.getByTestId("ai-role-fast").selectOption("default");
      await shot(page, `settings-two-${theme}-${size.width}`);
      await noOverflow(page, "ai-settings");
      await page.getByTestId("ai-save").click();
      await expect(dialog).toBeHidden();
      const s = await page.evaluate(() => (window as unknown as W).__rt.backend.ai.settings());
      expect(s?.profiles).toHaveLength(2);
      expect(s?.roles.fast).toBe("default");
      // The key never comes back.
      await command(page, "AI assistant: settings");
      await expect(page.getByTestId("ai-key")).toHaveValue("");
      await page.keyboard.press("Escape");
      await expect(dialog).toBeHidden();
    });
  }
}

test("preview: shows the masked call, Escape sends nothing, Send sends exactly it", async ({ page }) => {
  await open(page);
  await page.evaluate(async () => {
    const ai = (window as unknown as W).__rt.backend.ai as unknown as { secrets: string[] };
    ai.secrets = ["sk-demo-secret"];
    await (window as unknown as W).__rt.backend.ai.setSettings({
      profiles: [{ id: "default", label: "Claude", provider: "anthropic", model: "claude-opus-5-5", base_url: null }],
      roles: {},
      preview: "first-use",
    });
    await (window as unknown as W).__rt.backend.ai.setProfileKey("default", "k");
  });
  await command(page, "Demo: ask the AI");
  const preview = page.getByTestId("ai-preview");
  await expect(preview).toBeVisible();
  await expect(page.getByTestId("ai-preview-masked")).toContainText("replaced by ***");
  await expect(preview).not.toContainText("sk-demo-secret");
  await expect(preview.locator("mark").first()).toBeVisible();
  await shot(page, "preview");
  await page.getByTestId("ai-preview-toggle").click();
  await expect(page.getByTestId("ai-preview-raw")).not.toContainText("sk-demo-secret");
  await page.keyboard.press("Escape");
  await expect(preview).toBeHidden();
  await expect.poll(() => page.evaluate(() => (window as unknown as W).__demoAi?.error)).toBe("cancelled");
  expect(await page.evaluate(() => (window as unknown as W).__rt.backend.ai.sent.length)).toBe(0);

  await command(page, "Demo: ask the AI");
  await page.getByTestId("ai-preview-remember").check();
  await page.getByTestId("ai-preview-send").click();
  await expect.poll(() => page.evaluate(() => (window as unknown as W).__demoAi?.text)).toBeTruthy();
  const sent = await page.evaluate(() => JSON.stringify((window as unknown as W).__rt.backend.ai.sent));
  expect(sent).not.toContain("sk-demo-secret");
  expect((await page.evaluate(() => (window as unknown as W).__rt.backend.ai.settings()))?.preview).toBe("on-request");
  // Now it goes without a preview.
  await command(page, "Demo: ask the AI");
  await expect(page.getByTestId("ai-preview")).toHaveCount(0);
});

test("opt-out: a collection that turns the assistant off never builds a call", async ({ page }) => {
  await open(page);
  await page.evaluate(async () => {
    const b = (window as unknown as W).__rt.backend;
    await b.ai.setSettings({ profiles: [{ id: "default", label: "Claude", provider: "anthropic", model: "m", base_url: null }], roles: {}, preview: "always" });
    await b.ai.setProfileKey("default", "k");
  });
  await command(page, "AI assistant: settings");
  const allow = page.getByTestId("ai-allow");
  await expect(allow).toBeChecked();
  await allow.uncheck();
  await page.getByTestId("ai-cancel").click();
  await command(page, "Demo: ask the AI");
  await expect.poll(() => page.evaluate(() => (window as unknown as W).__demoAi?.error)).toBe("opted-out");
  await expect(page.getByTestId("ai-preview")).toHaveCount(0);
  expect(await page.evaluate(() => (window as unknown as W).__rt.backend.ai.sent.length)).toBe(0);
  // The team switch in the file wins and cannot be undone here.
  await page.evaluate(() => ((window as unknown as W).__rt.backend.ai as unknown as { fileOff: boolean }).fileOff = true);
  await command(page, "AI assistant: settings");
  await expect(allow).toBeDisabled();
  await expect(allow).not.toBeChecked();
  await shot(page, "opt-out-file");
});

test("off: without a profile nothing is offered and the call says not configured", async ({ page }) => {
  await open(page);
  await command(page, "Demo: ask the AI");
  await expect.poll(() => page.evaluate(() => (window as unknown as W).__demoAi?.error)).toBe("not-configured");
  await expect(page.getByTestId("ai-preview")).toHaveCount(0);
});
