// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand B: guided network access for extensions against the mock backend
// (`?backend=mock&extensions=egress`). A dev extension sends the request in `window.__egressRequest`
// through `ExtensionContext.egress`: the first call to a host asks once ("Allow <extension> to
// contact <host> for <purpose>?"), later calls do not, a refusal keeps the host out, the list
// "Egress hosts" shows who asked and why and takes a host back, a secret is named but never
// shown, and a guard that refuses comes back as `blocked`.

import { expect, test, type Page } from "@playwright/test";

const MOD = "ControlOrMeta";

type Result = { ok: true; value: { status: number; body: string; headers: [string, string][] } } | { ok: false; kind: string; message: string };

async function open(page: Page) {
  await page.goto("/?backend=mock&extensions=egress");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
}

async function call(page: Page, url: string, headers: [string, string][] = []) {
  await page.evaluate(
    ({ url, headers }) => {
      const w = window as unknown as { __egressRequest: unknown; __egressResult?: unknown };
      w.__egressResult = undefined;
      w.__egressRequest = { method: "GET", url, headers, extension: "Trace viewer", purpose: "Server traces" };
    },
    { url, headers },
  );
  await page.keyboard.press(`${MOD}+k`);
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await palette.getByRole("combobox").fill("Egress demo");
  await palette.getByRole("option").filter({ hasText: "Egress demo: send call" }).click();
}

const result = (page: Page) =>
  page.evaluate(async () => {
    const w = window as unknown as { __egressResult?: unknown };
    for (let i = 0; i < 100 && !w.__egressResult; i++) await new Promise((r) => setTimeout(r, 20));
    return w.__egressResult;
  }) as Promise<Result>;

test("the first call to a host asks once; the answer is remembered on the device", async ({ page }) => {
  await open(page);
  await call(page, "https://traces.example/api/traces/4bf9", [["Authorization", "Bearer {{secret:trace_token}}"]]);

  const ask = page.getByTestId("egress-ask");
  await expect(ask).toBeVisible();
  await expect(page.getByTestId("egress-ask-extension")).toHaveText("Trace viewer");
  await expect(page.getByTestId("egress-ask-host")).toHaveText("traces.example");
  await expect(page.getByTestId("egress-ask-purpose")).toHaveText("Server traces");
  // It says which secret goes to the host, by name.
  await expect(page.getByTestId("egress-ask-secrets")).toContainText("trace_token");
  await page.getByTestId("egress-allow").click();

  const first = await result(page);
  expect(first.ok).toBe(true);
  if (first.ok) {
    expect(first.value.status).toBe(200);
    expect(first.value.body).toContain("trace_token=");
    expect(first.value.body).not.toContain("Bearer");
  }

  // The second call goes straight through.
  await call(page, "https://traces.example/api/traces/other");
  expect((await result(page)).ok).toBe(true);
  await expect(page.getByTestId("egress-ask")).toHaveCount(0);
});

test("declining keeps the host out and the extension gets not-allowed", async ({ page }) => {
  await open(page);
  await call(page, "https://traces.example/x");
  await page.getByTestId("egress-deny").click();
  const answer = await result(page);
  expect(answer).toMatchObject({ ok: false, kind: "not-allowed" });
  // Asked again next time: nothing was stored.
  await call(page, "https://traces.example/x");
  await expect(page.getByTestId("egress-ask")).toBeVisible();
  await page.keyboard.press("Escape");
  expect(await result(page)).toMatchObject({ ok: false, kind: "not-allowed" });
});

test("the guard's refusal comes back as blocked", async ({ page }) => {
  await open(page);
  await call(page, "https://blocked.example/x");
  await page.getByTestId("egress-allow").click();
  const answer = await result(page);
  expect(answer).toMatchObject({ ok: false, kind: "blocked" });
});

test("Egress hosts lists who asked and why, and takes a host back", async ({ page }) => {
  await open(page);
  await call(page, "http://localhost:48706/x");
  await expect(page.getByTestId("egress-ask-host")).toHaveText("localhost:48706");
  await page.getByTestId("egress-allow").click();
  expect((await result(page)).ok).toBe(true);

  await page.keyboard.press(`${MOD}+k`);
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await palette.getByRole("combobox").fill("Egress hosts");
  await palette.getByRole("option").filter({ hasText: "Egress hosts" }).click();
  const row = page.getByTestId("egress-host");
  await expect(row).toHaveCount(1);
  await expect(row).toHaveAttribute("data-host", "localhost:48706");
  await expect(row).toContainText("Trace viewer · Server traces");
  await page.getByTestId("egress-remove").click();
  await expect(page.getByTestId("egress-host")).toHaveCount(0);
  await expect(page.getByText("No hosts allowed")).toBeVisible();
  await page.getByTestId("egress-hosts-done").click();

  // The host is out again: the same call asks again.
  await call(page, "http://localhost:48706/x");
  await expect(page.getByTestId("egress-ask")).toBeVisible();
});
