// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 8, strand B with the real core in the browser (rt-wasm) on an OPFS folder: a unary gRPC
// call through a real `rt agent` (`GET /v1/stream`), descriptors by server reflection over the
// same relay, the JSON message encoded to protobuf in the page and the reply decoded back,
// a secret inside the message masked in everything the page keeps. Without the companion the
// method picker says the companion is needed.
//
// Needs RT_BIN (the `rt` binary) and RT_TESTSERVER_BIN (for the gRPC echo server), e.g.
//   RT_BIN=$PWD/target/debug/rt RT_TESTSERVER_BIN=$PWD/target/debug/rt-testserver \
//     npm run e2e:shells -w apps/web

import { expect, test, type Page } from "@playwright/test";

import { startProcess } from "../git/server";
import { fakePicker, fillFolder, sampleCollection } from "./env";

/** Strand B's ports (47885-47888). */
const HTTP_PORT = Number(process.env.RT_GRPC_HTTP_PORT ?? 47886);
const GRPC_PORT = Number(process.env.RT_GRPC_PORT ?? 47887);
const AGENT_PORT = Number(process.env.RT_GRPC_AGENT_PORT ?? 47888);
const { RT_BIN, RT_TESTSERVER_BIN } = process.env;
const SECRET = ["grpc", "s3cret", "4712"].join("-");

type W = Window & { __rt: { backend: import("@requesttailor/ui").Backend } };

function collection(): Record<string, string> {
  return {
    ...sampleCollection(),
    "echo.yml": [
      "info:",
      "  name: Echo call",
      "  type: grpc",
      "  seq: 3",
      "",
      "grpc:",
      `  url: "grpc://127.0.0.1:${GRPC_PORT}"`,
      "  method: /rt.echo.v1.EchoService/Echo",
      "  methodType: unary",
      '  message: \'{"message": "hi {{token}}"}\'',
      "",
    ].join("\n"),
  };
}

async function open(page: Page, hash = "") {
  await fakePicker(page);
  await page.goto(`/${hash}`);
  await fillFolder(page, collection());
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Shell test");
  await page.evaluate(
    (secret) =>
      (window as unknown as W).__rt.backend.secrets.set(
        "Local",
        "token",
        secret,
      ),
    SECRET,
  );
  await page.getByTestId("env-switcher").selectOption("Local");
}

async function select(page: Page, name: string) {
  await page.getByTestId("view-requests").click();
  await page.getByTestId("tree-request").filter({ hasText: name }).click();
}

test("without the companion: reflection and the call say the companion is needed", async ({
  page,
}) => {
  await open(page);
  await select(page, "Echo call");
  const error = await page.evaluate(async () => {
    const b = (window as unknown as W).__rt.backend;
    return b.grpc.services("echo.yml", { environment: "Local", vars: [] }).then(
      () => "resolved",
      (e: { message: string }) => e.message,
    );
  });
  expect(error).toContain("needs the companion");
  await page.getByTestId("connect").click();
  await expect(page.getByTestId("session-status")).toContainText("companion");
});

test.describe("through a real rt agent", () => {
  test.skip(
    !RT_BIN || !RT_TESTSERVER_BIN,
    "needs RT_BIN and RT_TESTSERVER_BIN",
  );
  test.describe.configure({ mode: "serial" });

  let agent: ReturnType<typeof startProcess>;
  let server: ReturnType<typeof startProcess>;

  test.beforeAll(async ({}, info) => {
    server = startProcess(RT_TESTSERVER_BIN!, [
      "--port",
      String(HTTP_PORT),
      "--grpc-port",
      String(GRPC_PORT),
    ]);
    await server.line(/gRPC listening/);
    const origin = new URL(String(info.project.use.baseURL)).origin;
    agent = startProcess(RT_BIN!, [
      "agent",
      "--port",
      String(AGENT_PORT),
      "--allow-origin",
      origin,
    ]);
  });

  test.afterAll(() => {
    agent?.child.kill();
    server?.child.kill();
  });

  test("a unary call by reflection, decoded, the secret masked", async ({
    page,
  }) => {
    const code = (await agent.line(/#rt-pair=(\S+)/))[1];
    await page.addInitScript(
      ({ from, to }) => {
        const original = window.fetch.bind(window);
        window.fetch = (input: RequestInfo | URL, init?: RequestInit) =>
          original(
            typeof input === "string" && input.startsWith(from)
              ? to + input.slice(from.length)
              : input,
            init,
          );
        const fromWs = from.replace(/^http/, "ws");
        const toWs = to.replace(/^http/, "ws");
        const Native = window.WebSocket;
        window.WebSocket = class extends Native {
          constructor(url: string | URL, protocols?: string | string[]) {
            const target = String(url);
            super(
              target.startsWith(fromWs)
                ? toWs + target.slice(fromWs.length)
                : target,
              protocols,
            );
          }
        };
      },
      { from: "http://127.0.0.1:47812", to: `http://127.0.0.1:${AGENT_PORT}` },
    );
    await open(page, `#rt-pair=${code}`);
    await page.evaluate(() =>
      (window as unknown as W).__rt.backend.transport.useCompanion(true),
    );
    await select(page, "Echo call");

    // The method picker lists the services by reflection through the companion.
    await page.getByTestId("tab-messages").click();
    await expect(page.getByTestId("grpc-method-select")).toContainText("Echo");
    await expect(page.getByTestId("grpc-method-type")).toContainText(/unary/i);

    await page.getByTestId("connect").click();
    await expect(page.getByTestId("session-status")).toHaveText("Connected");
    await page.getByTestId("session-send").click();
    await expect(page.getByTestId("session-status")).toHaveText(
      "Closed by the server (0)",
    );
    const rows = page.getByTestId("stream-row");
    await expect(rows.filter({ hasText: "hi ***" })).toHaveCount(2);
    await expect(rows.locator('[aria-label="received"]')).toHaveCount(1);
    await expect(page.getByTestId("session-pane")).not.toContainText(SECRET);

    const log = await page.evaluate(async () => {
      const b = (window as unknown as W).__rt.backend;
      const id = await b.streams.open(
        "echo.yml",
        { environment: "Local", vars: [] },
        () => {},
      );
      await b.streams.send(id, {
        title: "",
        selected: true,
        kind: "json",
        data: '{"message": "again {{token}}"}',
      });
      let log = await b.streams.log(id);
      for (
        let i = 0;
        i < 200 && !log.events.some((e) => e.event === "closed");
        i++
      ) {
        await new Promise((r) => setTimeout(r, 20));
        log = await b.streams.log(id);
      }
      await b.streams.release(id);
      return JSON.stringify(log);
    });
    expect(log).toContain("again ***");
    expect(log).toContain('"name":"rt.echo.v1.EchoReply"');
    expect(log).not.toContain(SECRET);
    expect(agent.stderr()).not.toContain(SECRET);
  });
});
