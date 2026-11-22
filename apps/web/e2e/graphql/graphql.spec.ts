// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Acceptance tests of wave 8, strand B (GraphQL editor, gRPC method picker), against the
// mock backend (its schema is the library of rt-testserver's `POST /graphql`).

import { expect, test, type Locator, type Page } from "@playwright/test";

import type { Backend, RequestSpec } from "@requesttailor/ui";

/** The page's backend (the mock counts introspection fetches). */
type W = Window & {
  __rt: { backend: Backend & { graphql: { fetched: number } } };
};

const cm = (field: Locator) => field.locator(".cm-content");

async function openGraphQl(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await page
    .getByTestId("tree-request")
    .filter({ hasText: "Create user" })
    .click();
  await page.getByTestId("tab-body").click();
  await page.getByTestId("body-type").selectOption("graphql");
  await expect(page.getByTestId("graphql-editor")).toBeVisible();
  await expect(page.getByTestId("graphql-schema-status")).toHaveText(
    "Schema loaded",
  );
}

const backendSpec = (page: Page) =>
  page.evaluate(() =>
    (window as unknown as W).__rt.backend.getRequest("users/create-user.yml"),
  ) as Promise<RequestSpec>;

test("the GraphQL body edits query and variables", async ({ page }) => {
  await openGraphQl(page);
  const query = cm(page.getByTestId("graphql-query"));
  await query.click();
  await page.keyboard.type("{ hello }");
  await cm(page.getByTestId("graphql-variables")).click();
  await page.keyboard.type('{"a": 1}');
  await expect
    .poll(async () => (await backendSpec(page)).body)
    .toEqual({ type: "graphql", query: "{ hello }", variables: '{"a": 1}' });
  await page.keyboard.press("Backspace");
  await expect(page.getByTestId("graphql-variables-problem")).toContainText(
    "not valid JSON",
  );
});

test("completion and validation come from the schema", async ({ page }) => {
  await openGraphQl(page);
  const query = cm(page.getByTestId("graphql-query"));
  await query.click();
  await page.keyboard.type("{ boo");
  const labels = page.locator(".cm-tooltip-autocomplete .cm-completionLabel");
  await expect(labels.filter({ hasText: /^books$/ })).toBeVisible();
  await expect(labels.filter({ hasText: /^book$/ })).toBeVisible();
  await page.keyboard.press("Escape");
  await page.keyboard.type("ks { nope } }");
  await expect(page.locator(".cm-lintRange-error")).toHaveCount(1, {
    timeout: 5000,
  });
  await page.locator(".cm-lintRange-error").hover();
  await expect(page.locator(".cm-tooltip-lint")).toContainText(
    'Cannot query field "nope" on type "Book".',
  );
});

test("the schema browser lists and searches types; refresh asks again", async ({
  page,
}) => {
  await openGraphQl(page);
  expect(
    await page.evaluate(
      () => (window as unknown as W).__rt.backend.graphql.fetched,
    ),
  ).toBe(1);
  await page.getByTestId("graphql-schema-toggle").click();
  const browser = page.getByTestId("graphql-schema-browser");
  await expect(browser.getByTestId("graphql-schema-type")).toHaveText([
    /query\s*Query/,
    /mutation\s*Mutation/,
    /object\s*Book/,
    /input\s*BookInput/,
    /enum\s*Genre/,
  ]);
  await browser.getByTestId("graphql-schema-search").fill("deprecated");
  await expect(browser.getByTestId("graphql-schema-type")).toHaveCount(0);
  await browser.getByTestId("graphql-schema-search").fill("year");
  const year = browser.locator('[data-field="year"]');
  await expect(year).toContainText("year: Int");
  await expect(year).toContainText("Deprecated: Use published.");
  await page.getByTestId("graphql-schema-refresh").click();
  await expect
    .poll(() =>
      page.evaluate(
        () => (window as unknown as W).__rt.backend.graphql.fetched,
      ),
    )
    .toBe(2);
});

test("the gRPC picker sets the method type and fills the template", async ({
  page,
}) => {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await page.getByTestId("new-request").click();
  await page.getByTestId("new-request-kind").selectOption("grpc");
  const name = page.getByTestId("new-request-name");
  await name.fill("Countdown");
  await name.press("Enter");
  await page.getByTestId("tab-messages").click();
  const picker = page.getByTestId("grpc-picker");
  await picker
    .getByTestId("grpc-method-select")
    .selectOption("/rt.echo.v1.EchoService/Countdown");
  await expect(picker.getByTestId("grpc-method-type")).toHaveText(
    "Server streaming",
  );
  await expect(picker.getByTestId("grpc-method-io")).toHaveText(
    "rt.echo.v1.CountdownRequest → rt.echo.v1.EchoReply",
  );
  await picker.getByTestId("grpc-use-template").click();
  await expect(page.getByTestId("message-editor")).toContainText('"count": 0');
});
