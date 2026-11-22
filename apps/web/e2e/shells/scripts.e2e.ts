// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 7, strand A: scripts in the web app with the real core. The trust banner, a send from
// an untrusted collection, and an endless loop that must neither freeze the page nor run past
// the time limit.

import { expect, test, type Page } from "@playwright/test";

import { SERVER, fakePicker, fillFolder } from "./env";

function request(name: string, seq: number, url: string, script?: string): string {
  const lines = ["info:", `  name: ${name}`, "  type: http", `  seq: ${seq}`, "", "http:", "  method: GET", `  url: "${url}"`];
  if (script) lines.push("", "runtime:", "  scripts:", "    - type: before-request", `      code: ${script}`);
  return lines.join("\n") + "\n";
}

function scriptCollection(): Record<string, string> {
  return {
    "opencollection.yml": 'opencollection: "1.0.0"\ninfo:\n  name: Script test\n',
    "environments/Local.yml": `name: Local\nvariables:\n  - name: baseUrl\n    value: ${SERVER}\n`,
    "loop.yml": request("Loop", 1, "{{baseUrl}}/echo/loop", "while (true) {}"),
    // The script's effect shows in the status: 201 when it ran, 200 when not.
    "header.yml": request("Header", 2, "{{baseUrl}}/status/200", "rt.request.url = rt.request.url.replace('200', '201');"),
    "plain.yml": request("Plain", 3, "{{baseUrl}}/echo/plain"),
  };
}

async function open(page: Page): Promise<void> {
  await fakePicker(page);
  await page.goto("/");
  await fillFolder(page, scriptCollection());
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Script test");
  await page.getByTestId("env-switcher").selectOption("Local");
}

const item = (page: Page, name: string) => page.getByTestId("tree-request").filter({ hasText: name });

test("untrusted: the banner asks, sends run without scripts and say so", async ({ page }) => {
  await open(page);
  const banner = page.getByTestId("script-trust");
  await expect(banner).toContainText("This collection contains scripts. Run them?");
  // The banner takes no focus: sending stays at the same keystrokes.
  await expect(banner.locator(":focus")).toHaveCount(0);

  await page.getByTestId("script-trust-later").click();
  await expect(banner).toBeHidden();
  await item(page, "Header").click();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");
  const note = page.getByTestId("send-note").filter({ hasText: "not trusted" });
  await expect(note).toHaveAttribute("data-source", "script");
  await expect(note).toHaveAttribute("data-level", "warning");

  // A request without scripts gets no note.
  await item(page, "Plain").click();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");
  await expect(page.getByTestId("send-note")).toHaveCount(0);
});

test("trusted: scripts run, an endless loop keeps the page responsive and stops at 5 s", async ({ page }) => {
  test.setTimeout(60_000);
  await open(page);
  await page.getByTestId("script-trust-allow").click();
  await expect(page.getByTestId("script-trust")).toBeHidden();

  // Scripts run now.
  await item(page, "Header").click();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("201");
  await expect(page.getByTestId("send-note")).toHaveCount(0);

  // Frame gaps while the loop runs.
  await page.evaluate(() => {
    const w = window as unknown as { __gaps: number[]; __stop: boolean };
    w.__gaps = [];
    w.__stop = false;
    let last = performance.now();
    const frame = (t: number) => {
      w.__gaps.push(t - last);
      last = t;
      if (!w.__stop) requestAnimationFrame(frame);
    };
    requestAnimationFrame(frame);
  });
  await item(page, "Loop").click();
  const started = Date.now();
  await page.getByTestId("send").click();

  // The page still reacts: another request opens while the script spins.
  await page.waitForTimeout(800);
  await item(page, "Plain").click();
  await expect(page.getByTestId("url-input")).toContainText("/echo/plain", { timeout: 1_000 });
  await page.waitForTimeout(500);

  await item(page, "Loop").click();
  await expect(page.getByTestId("response-error")).toContainText("Script stopped: time limit of 5 s exceeded", {
    timeout: 15_000,
  });
  const elapsed = Date.now() - started;
  const gaps = await page.evaluate(() => {
    const w = window as unknown as { __gaps: number[]; __stop: boolean };
    w.__stop = true;
    return w.__gaps;
  });
  const sorted = [...gaps].sort((a, b) => a - b);
  const p95 = sorted[Math.floor(sorted.length * 0.95)];
  console.log(`endless loop: ${elapsed} ms to the error, ${gaps.length} frames, p95 gap ${p95.toFixed(1)} ms`);
  expect(gaps.length).toBeGreaterThan(50);
  expect(p95).toBeLessThanOrEqual(100);
  expect(elapsed).toBeGreaterThanOrEqual(4_500);
  expect(elapsed).toBeLessThan(10_000);
});
