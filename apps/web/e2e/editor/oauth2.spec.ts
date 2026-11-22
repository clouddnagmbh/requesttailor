// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 5, strand C: the OAuth 2.0 editor, the sign-in prompt and the token overview, against
// the mock backend (its `fetchToken` plays a browser or device code flow).

import { expect, test, type Locator, type Page } from "@playwright/test";

import type { Auth, Backend, RequestSpec } from "@requesttailor/ui";

/** The web app exposes its backend for tests (`src/main.ts`). Inlined in each `evaluate`,
 *  which runs in the page and cannot see this module. */
type Rt = { __rt: { backend: Backend } };

const LIST = "users/list-users.yml";

async function openRequest(page: Page, before?: () => Promise<void>) {
  await page.goto("/?backend=mock");
  if (before) await before();
  await page.getByTestId("open-collection").click();
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
  await expect(page.getByTestId("request-editor")).toBeVisible();
  await page.getByTestId("tab-auth").click();
}

async function setText(field: Locator, text: string) {
  const content = field.locator(".cm-content");
  await content.click();
  await content.press("ControlOrMeta+a");
  if (text === "") await content.press("Backspace");
  else await content.page().keyboard.type(text);
}

const backendAuth = (page: Page) =>
  page.evaluate(async (p) => (await (window as unknown as Rt).__rt.backend.getRequest(p)).auth, LIST) as Promise<Auth>;

async function expectAuth(page: Page, check: (a: Auth) => unknown, expected: unknown) {
  await expect.poll(async () => check(await backendAuth(page))).toEqual(expected);
}

function withAuth(page: Page, auth: Auth) {
  return () =>
    page.evaluate(
      async ([p, a]) => {
        const b = (window as unknown as Rt).__rt.backend;
        await b.openCollection();
        const s: RequestSpec = await b.getRequest(p);
        await b.updateRequest(p, { ...s, auth: a });
      },
      [LIST, auth] as const,
    );
}

test("every OAuth 2.0 field is edited through the Auth tab", async ({ page }) => {
  await openRequest(page);
  await page.getByTestId("auth-type").selectOption("oauth2");
  await expect(page.getByTestId("oauth2-flow")).toHaveValue("client-credentials");

  await setText(page.getByTestId("oauth2-token-url"), "{{idp}}/token");
  await setText(page.getByTestId("oauth2-client-id"), "app");
  await setText(page.getByTestId("oauth2-client-secret"), "{{clientSecret}}");
  await page.getByTestId("oauth2-credentials-placement").selectOption("body");
  await setText(page.getByTestId("oauth2-scope"), "openid profile");
  await expectAuth(
    page,
    (a) => a.type === "oauth2" && [a.flow, a.access_token_url, a.client_id, a.client_secret, a.credentials_placement, a.scope],
    ["client-credentials", "{{idp}}/token", "app", "{{clientSecret}}", "body", "openid profile"],
  );

  // Authorization code keeps the client and starts with PKCE S256.
  await page.getByTestId("oauth2-flow").selectOption("authorization-code");
  await setText(page.getByTestId("oauth2-authorization-url"), "{{idp}}/auth");
  await setText(page.getByTestId("oauth2-callback-url"), "http://127.0.0.1:8765/callback");
  await expectAuth(
    page,
    (a) => a.type === "oauth2" && a.flow === "authorization-code" && [a.authorization_url, a.callback_url, a.pkce, a.client_id],
    ["{{idp}}/auth", "http://127.0.0.1:8765/callback", "S256", "app"],
  );
  await page.getByTestId("oauth2-pkce").selectOption("off");
  await expectAuth(page, (a) => a.type === "oauth2" && a.flow === "authorization-code" && a.pkce, null);

  // Extra parameters stay grouped by step.
  await page.getByTestId("oauth2-param-add-refresh").click();
  await page.getByTestId("oauth2-param-add-authorization").click();
  await setText(page.getByTestId("oauth2-param-name").first(), "audience");
  await expectAuth(
    page,
    (a) => a.type === "oauth2" && a.parameters.map((p) => [p.step, p.name, p.placement]),
    [
      ["authorization", "audience", "query"],
      ["refresh", "", "body"],
    ],
  );

  // Token placement and switches.
  await page.getByTestId("oauth2-token-placement").selectOption("query");
  await setText(page.getByTestId("oauth2-token-query"), "token");
  await page.getByTestId("oauth2-token-source").selectOption("id-token");
  await page.getByTestId("oauth2-auto-refresh").uncheck();
  await expectAuth(
    page,
    (a) => a.type === "oauth2" && [a.token.placement, a.token.source, a.auto_fetch, a.auto_refresh],
    [{ in: "query", name: "token" }, "id-token", true, false],
  );

  // Implicit is stored, with a note that it is not executed.
  await page.getByTestId("oauth2-flow").selectOption("implicit");
  await expect(page.getByTestId("oauth2-implicit-note")).toBeVisible();
  await expect(page.getByTestId("oauth2-get-token")).toBeDisabled();
  await expect(page.getByTestId("oauth2-token-url")).toHaveCount(0);
});

test("Get new token shows the browser prompt, the token status and the token overview", async ({ page }) => {
  await openRequest(page);
  await page.getByTestId("auth-type").selectOption("oauth2");
  await page.getByTestId("oauth2-get-token").click();

  const prompt = page.getByTestId("auth-prompt");
  await expect(prompt).toBeVisible();
  await expect(prompt.getByRole("heading")).toHaveText("Sign in to get a token");
  await expect(page.getByTestId("auth-prompt-url")).toContainText("https://idp.example/authorize");
  await expect(page.getByTestId("auth-prompt-open")).toHaveAttribute("href", /^https:\/\/idp\.example\/authorize/);
  await expect(prompt).toBeHidden();
  await expect(page.getByTestId("oauth2-token-status")).toContainText("Token received");
  await expect(page.getByTestId("oauth2-token-status")).toContainText("refreshable");

  await page.getByTestId("oauth2-tokens").click();
  const dialog = page.getByTestId("tokens-dialog");
  await expect(dialog.getByTestId("tokens-row")).toHaveCount(1);
  await expect(dialog.getByTestId("tokens-row")).toContainText("idp.example · mock-client");
  await dialog.getByTestId("tokens-delete").click();
  await expect(dialog.getByTestId("tokens-empty")).toBeVisible();
  await dialog.getByTestId("tokens-close").click();
  await expect(dialog).toBeHidden();
});

test("cancelling the prompt stops the flow", async ({ page }) => {
  await openRequest(page);
  await page.getByTestId("auth-type").selectOption("oauth2");
  await page.getByTestId("oauth2-get-token").click();
  await expect(page.getByTestId("auth-prompt")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("auth-prompt")).toBeHidden();
  await expect(page.getByTestId("oauth2-token-status")).toContainText("cancelled");
});

test("a device code prompt shows the code and the verification link", async ({ page }) => {
  await openRequest(page);
  const done = page.evaluate(() =>
    (window as unknown as Rt).__rt.backend.auth.fetchToken("device/login.yml", { environment: null, vars: [] }).then((t) => t.key),
  );
  await expect(page.getByTestId("auth-prompt-user-code")).toHaveText("WDJB-MJHT");
  await expect(page.getByTestId("auth-prompt-verification")).toHaveAttribute("href", "https://idp.example/device");
  await expect(page.getByTestId("auth-prompt-verification-complete")).toHaveAttribute("href", /user_code=WDJB-MJHT/);
  await expect(page.getByTestId("auth-prompt-expires")).toContainText(/Expires in (10:00|9:5\d)/);
  expect(await done).toBe("mock|device/login.yml");
  await expect(page.getByTestId("auth-prompt")).toBeHidden();
});

test("device code and SAML bearer are shown as collection defaults only", async ({ page }) => {
  const auth = {
    type: "oauth2",
    flow: "device-code",
    device_authorization_url: "https://idp/device",
    access_token_url: "https://idp/token",
    refresh_token_url: "",
    client_id: "cli",
    client_secret: "",
    credentials_placement: "body",
    scope: "",
    parameters: [],
    token: { id: "", placement: { in: "header", prefix: "Bearer" }, source: "access-token" },
    auto_fetch: true,
    auto_refresh: true,
  } satisfies Auth;
  await openRequest(page, withAuth(page, auth));
  await expect(page.getByTestId("oauth2-flow")).toHaveValue("device-code");
  await expect(page.getByTestId("oauth2-collection-only-note")).toContainText("opencollection.yml");
  // Not offered once the request uses another flow.
  await page.getByTestId("oauth2-flow").selectOption("client-credentials");
  await expect(page.getByTestId("oauth2-flow").locator("option[value=device-code]")).toHaveCount(0);
});
