// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Documentation screenshots, set "Free II": git, protocols, agents, MCP client, AI, OData and
// plugins. Runs only with RT_DOC_SHOTS=<dir> (writes <id>.png there); skipped otherwise.
//   RT_DOC_SHOTS=/tmp/shots RT_WEB_PORT=48910 npx playwright test e2e/docshots/free-2.spec.ts
// Mock backend only; every host is example.com or loopback, every name invented.

import { spawn } from "node:child_process";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { expect, test, type Locator, type Page } from "@playwright/test";

import type { MockBackend } from "@requesttailor/ui";

type MockGit = import("../../../ui/src/commands/mock-git").MockGit;
type W = Window & { __rt: { backend: MockBackend & { git: MockGit } } };

const DIR = process.env.RT_DOC_SHOTS;
const MOD = "ControlOrMeta";

test.skip(!DIR, "set RT_DOC_SHOTS=<dir> to write the documentation screenshots");
test.setTimeout(120_000);
test.use({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1, colorScheme: "light" });

async function shot(page: Page, id: string, clip?: { x: number; y: number; width: number; height: number }) {
  await page.mouse.move(1430, 890);
  await page.waitForTimeout(250);
  await page.screenshot({ path: `${DIR}/${id}.png`, ...(clip ? { clip } : {}) });
}

async function open(page: Page, query = "") {
  await page.goto(`/?backend=mock${query}`);
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
}

async function command(page: Page, label: string) {
  await page.keyboard.press(`${MOD}+k`);
  await page.getByTestId("palette-input").fill(label);
  await page.getByTestId("palette-option").filter({ hasText: label }).first().click();
}

const cm = (field: Locator) => field.locator(".cm-content");

async function setText(field: Locator, text: string) {
  const content = cm(field);
  await content.click();
  await content.press("ControlOrMeta+a");
  if (text === "") await content.press("Backspace");
  else await content.page().keyboard.type(text);
}

async function identity(page: Page) {
  await page.evaluate(async () => {
    await (window as unknown as W).__rt.backend.git.setIdentity({ name: "Ada Example", email: "ada@example.com" });
  });
}

async function changeAndSave(page: Page, name: string, method: string) {
  await page.getByTestId("view-requests").click();
  await page.getByTestId("tree-request").filter({ hasText: name }).click();
  await page.getByTestId("method-select").selectOption(method);
  await page.getByTestId("save").click();
  await expect(page.getByTestId("save")).toBeDisabled();
}

/** A wider sidebar (device setting), so the Git panel does not cut its labels. */
async function wideSidebar(page: Page, width = 340) {
  await page.addInitScript((w) => {
    try {
      localStorage.setItem("rt.shell.sidebarWidth", JSON.stringify(w));
    } catch {
      // No storage: default width.
    }
  }, width);
}

async function gitScenario(page: Page, scenario: string) {
  await page.evaluate(async (scenario) => {
    const git = (window as unknown as W).__rt.backend.git;
    git.scenario(scenario as never);
    await git.setIdentity({ name: "Ada Example", email: "ada@example.com" });
  }, scenario);
}

async function openGitPanel(page: Page, scenario: string) {
  await wideSidebar(page);
  await open(page);
  await gitScenario(page, scenario);
  await page.getByTestId("view-git").click();
  await expect(page.getByTestId("git-panel")).toBeVisible();
  await expect(page.getByTestId("git-panel")).not.toHaveAttribute("data-situation", "loading");
}

test.describe("git", () => {
  test("git-aenderungen", async ({ page }) => {
    await wideSidebar(page);
    await open(page);
    await identity(page);
    await changeAndSave(page, "Health", "HEAD");
    await changeAndSave(page, "100 MB response", "HEAD");
    await page.getByTestId("view-git").click();
    await expect(page.getByTestId("git-panel")).toBeVisible();
    await page.locator('[data-testid="git-file"][data-path="health.yml"]').getByTestId("git-file-open").click();
    await expect(page.getByTestId("git-diff")).toBeVisible();
    await shot(page, "git-aenderungen");
  });

  test("git-commit", async ({ page }) => {
    await wideSidebar(page);
    await open(page);
    await identity(page);
    await changeAndSave(page, "Health", "HEAD");
    await changeAndSave(page, "100 MB response", "HEAD");
    await page.getByTestId("view-git").click();
    await expect(page.getByTestId("git-panel")).toBeVisible();
    await page.getByTestId("git-message").fill("Health check uses HEAD");
    await shot(page, "git-commit");
  });

  test("git-graph", async ({ page }) => {
    await open(page);
    await gitScenario(page, "showcase");
    await command(page, "Git: History");
    await expect(page.getByTestId("git-view")).toHaveAttribute("data-kind", "graph");
    await expect(page.getByTestId("graph-row").first()).toBeVisible();
    await page.getByTestId("graph-row").filter({ hasText: "Move user creation to v2" }).click();
    await expect(page.getByTestId("graph-semantic-diff")).toBeVisible();
    await shot(page, "git-graph");
  });

  test("git-semantisch", async ({ page }) => {
    await wideSidebar(page);
    await open(page);
    await identity(page);
    await changeAndSave(page, "Health", "HEAD");
    await page.getByTestId("view-requests").click();
    await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
    await setText(page.getByTestId("url-input"), "{{baseUrl}}/v2/users?page=1");
    await page.getByTestId("save").click();
    await expect(page.getByTestId("save")).toBeDisabled();
    await page.getByTestId("view-git").click();
    await page.getByTestId("git-semantic-diff").click();
    await expect(page.getByTestId("semantic-diff-view")).toBeVisible();
    await expect(page.getByTestId("sd-change")).toHaveCount(2);
    await shot(page, "git-semantisch");
  });

  test("git-vergleich", async ({ page }) => {
    await open(page);
    await page.evaluate(async () => {
      const b = (window as unknown as W).__rt.backend;
      await b.git.setIdentity({ name: "Ada Example", email: "ada@example.com" });
      const edit = async (path: string, message: string, change: (s: Awaited<ReturnType<typeof b.getRequest>>) => object) => {
        const spec = await b.getRequest(path);
        await b.updateRequest(path, { ...spec, ...change(spec) });
        await b.save();
        await b.git.commit(message, [path]);
      };
      await edit("health.yml", "Assert 200", () => ({
        assertions: [{ expression: "res.status", operator: "eq", value: "200", description: null, enabled: true }],
      }));
      await b.git.createBranch("feature/pets", true);
      await edit("health.yml", "Health moved", () => ({ url: "{{baseUrl}}/status/503" }));
      await edit("users/list-users.yml", "Second page", (s) => ({ url: "{{baseUrl}}/users?page=2", params: s.params.map((p) => ({ ...p, value: "2" })) }));
      const pets = await b.createRequest("", "Pets");
      const spec = await b.getRequest(pets);
      await b.updateRequest(pets, { ...spec, url: "{{baseUrl}}/pets" });
      await b.save();
      await b.git.commit("Add pets", [pets]);
    });
    await command(page, "Compare revisions");
    await expect(page.getByTestId("compare-panel")).toBeVisible();
    for (const [side, rev] of [["from", "main"], ["to", "feature/pets"]] as const) {
      await page.getByTestId(`compare-${side}`).click();
      await page.getByTestId(`compare-${side}-list`).locator(`[role="option"][data-rev="${rev}"]`).first().click();
    }
    await page.getByTestId("compare-start").click();
    await expect(page.getByTestId("compare-count-changed").locator(".value")).toHaveText("2");
    await page.getByTestId("compare-row").filter({ hasText: "Health" }).click();
    await expect(page.getByTestId("cmp-new-side").getByTestId("cmp-status")).toHaveText("503");
    await shot(page, "git-vergleich");
  });

  test("git-assistent", async ({ page }) => {
    await openGitPanel(page, "behind");
    await page.getByTestId("git-branch").click();
    await page.getByRole("menu").getByRole("menuitem", { name: "Merge into main…" }).click();
    await page.getByTestId("git-wizard-from").selectOption("origin/main");
    await expect(page.getByTestId("git-wizard-plan")).toBeVisible();
    await expect(page.getByTestId("git-wizard-merge")).toBeVisible();
    await shot(page, "git-assistent");
  });
});

async function createRequest(page: Page, name: string, kind: string) {
  await page.getByTestId("new-request").click();
  await page.getByTestId("new-request-kind").selectOption(kind);
  const field = page.getByTestId("new-request-name");
  await field.fill(name);
  await field.press("Enter");
  await expect(page.getByTestId("tab").filter({ hasText: name })).toHaveAttribute("aria-selected", "true");
}

test.describe("protocols", () => {
  test("protokolle-grpc", async ({ page }) => {
    await open(page);
    await createRequest(page, "Countdown", "grpc");
    await setText(page.getByTestId("url-input"), "https://grpc.example.com:443");
    await page.getByTestId("tab-messages").click();
    const picker = page.getByTestId("grpc-picker");
    await picker.getByTestId("grpc-method-select").selectOption("/rt.echo.v1.EchoService/Countdown");
    await picker.getByTestId("grpc-use-template").click();
    await setText(page.getByTestId("message-data"), '{"count": 3}');
    await page.getByTestId("connect").click();
    await expect(page.getByTestId("session-status")).toContainText("Connected");
    await page.getByTestId("session-send").click();
    await expect(page.getByTestId("stream-row").first()).toBeVisible();
    await page.waitForTimeout(1500);
    await shot(page, "protokolle-grpc");
  });

  test("protokolle-websocket", async ({ page }) => {
    await open(page);
    await createRequest(page, "Echo socket", "websocket");
    await setText(page.getByTestId("url-input"), "wss://echo.example.com/ws");
    await page.getByTestId("tab-messages").click();
    await setText(page.getByTestId("message-data"), '{"hello":"world"}');
    await page.getByTestId("message-add").click();
    await setText(page.getByTestId("message-data"), "ping");
    await page.getByTestId("connect").click();
    await expect(page.getByTestId("session-status")).toHaveText("Connected");
    await page.getByTestId("session-send").click();
    await page.getByTestId("message-variant").filter({ hasText: "Message 1" }).click();
    await page.getByTestId("session-send").click();
    await expect(page.getByTestId("stream-row").filter({ hasText: "hello" })).toHaveCount(2);
    await shot(page, "protokolle-websocket");
  });

  test("protokolle-sse", async ({ page }) => {
    await open(page);
    await createRequest(page, "Ticks", "http");
    await setText(page.getByTestId("url-input"), "https://api.example.com/events");
    await page.getByTestId("tab-headers").click();
    const table = page.getByTestId("headers-table");
    await table.getByTestId("kv-add").click();
    const row = table.getByTestId("kv-row").nth(0);
    await setText(row.getByTestId("kv-name"), "Accept");
    await setText(row.getByTestId("kv-value"), "text/event-stream");
    await page.getByTestId("connect").click();
    await expect(page.getByTestId("stream-row").filter({ hasText: "tick#" })).toHaveCount(3);
    await shot(page, "protokolle-sse");
  });

  test("protokolle-graphql", async ({ page }) => {
    await open(page);
    await createRequest(page, "List books", "graphql");
    await setText(page.getByTestId("url-input"), "https://api.example.com/graphql");
    await page.getByTestId("tab-body").click();
    await expect(page.getByTestId("graphql-editor")).toBeVisible();
    await expect(page.getByTestId("graphql-schema-status")).toHaveText("Schema loaded");
    await setText(page.getByTestId("graphql-query"), "query { books { title year } }");
    await page.keyboard.press("Escape");
    await page.getByTestId("graphql-schema-toggle").click();
    await page.getByTestId("graphql-schema-search").fill("year");
    await expect(page.locator('[data-field="year"]')).toContainText("year: Int");
    await shot(page, "protokolle-graphql");
  });
});

type AgentsW = {
  __rt: {
    backend: MockBackend & {
      agents: MockBackend["agents"] & { request(p: object): { id: string }; decisions: { id: string; decision: string }[] };
    };
  };
};

async function agentRequests(page: Page, requests: object[], decide: ("approved" | "denied" | null)[]) {
  await page.evaluate(
    async ({ requests, decide }) => {
      const agents = (window as unknown as AgentsW).__rt.backend.agents;
      const made = requests.map((r) => agents.request(r));
      for (const [i, d] of decide.entries()) if (d) await agents.decide(made[i]!.id, d);
    },
    { requests, decide },
  );
}

const CREATE = { agent: "claude-code", method: "POST", url: "https://api.example.com/users?api_key=****", environment: "PRD", request_path: "users/create-user.yml" };

test.describe("agents", () => {
  test("agenten-freigabe", async ({ page }) => {
    await open(page);
    await page.getByTestId("tree-request").filter({ hasText: "Create user" }).click();
    await agentRequests(page, [CREATE], [null]);
    await expect(page.getByTestId("agent-approval")).toHaveCount(1);
    await expect(page.getByTestId("request-editor")).toBeVisible();
    await shot(page, "agenten-freigabe");
  });

  test("agenten-regeln", async ({ page }) => {
    await open(page);
    await page.getByTestId("tree-request").filter({ hasText: "Create user" }).click();
    await command(page, "Agent access");
    const editor = page.getByTestId("agent-policy");
    await expect(editor).toBeVisible();
    await editor.getByTestId("agent-policy-default").selectOption("write");
    const prd = editor.locator('[data-testid="agent-policy-row"][data-environment="PRD"]');
    await prd.getByTestId("agent-policy-production").check();
    await expect(prd.getByTestId("agent-policy-effective")).toHaveText("Writing requests wait for approval");
    await shot(page, "agenten-regeln");
  });

  test("agenten-audit", async ({ page }) => {
    await open(page);
    await page.getByTestId("tree-request").filter({ hasText: "Create user" }).click();
    await agentRequests(
      page,
      [
        CREATE,
        { agent: "cursor", method: "DELETE", url: "https://api.example.com/users/7", environment: "PRD", request_path: "users/delete-user.yml" },
        { agent: "claude-code", method: "PATCH", url: "https://api.example.com/users/8", environment: "PRD", request_path: "users/update-user.yml" },
      ],
      ["approved", "denied", "approved"],
    );
    await command(page, "Agent audit log");
    await expect(page.getByTestId("agent-audit-row")).toHaveCount(3);
    await shot(page, "agenten-audit");
  });
});

const mcpTool = (page: Page, name: string) => page.locator(`[data-testid="mcp-tool"][data-name="${name}"]`);

async function openMcp(page: Page) {
  await open(page);
  await command(page, "Test an MCP");
  await expect(page.getByTestId("mcp-panel")).toBeVisible();
}

async function callAdd(page: Page) {
  await page.getByTestId("mcp-url").fill("http://localhost:3333/mcp");
  await page.getByTestId("mcp-connect").click();
  await expect(page.getByTestId("mcp-tools")).toHaveAttribute("data-count", "4");
  await mcpTool(page, "add").click();
  await page.getByTestId("mcp-args-raw").click();
  await page.getByTestId("mcp-args-json").fill('{"a": 2, "b": 3}');
  await page.getByTestId("mcp-call").click();
  await expect(page.getByTestId("mcp-result-structured")).toContainText('"sum": 5');
}

test.describe("mcp client", () => {
  test("mcp-client-server", async ({ page }) => {
    await openMcp(page);
    await callAdd(page);
    await page.getByTestId("mcp-about").click();
    await shot(page, "mcp-client-server");
  });

  test("mcp-client-test", async ({ page }) => {
    await openMcp(page);
    await callAdd(page);
    await page.getByTestId("mcp-save-test").click();
    await page.getByTestId("mcp-test-name").fill("Add numbers");
    await page.getByTestId("mcp-save-confirm").click();
    const saved = page.getByTestId("mcp-test");
    await expect(saved).toHaveAttribute("data-path", "mcp/add-numbers.mcp.yaml");
    await page.keyboard.press(`${MOD}+s`);
    await saved.getByTestId("mcp-test-run").click();
    await expect(saved).toHaveAttribute("data-state", "passed");
    await expect(page.getByTestId("mcp-tests-summary")).toContainText("1 passed");
    await page.getByTestId("mcp-about").click();
    await shot(page, "mcp-client-test");
  });

  test("mcp-client-demo", async ({ page }) => {
    await open(page);
    await page.getByTestId("view-mcp").click();
    await page.getByTestId("mcp-try-demo").click();
    await expect(page.getByTestId("mcp-tools")).toHaveAttribute("data-count", "10");
    await mcpTool(page, "countdown").click();
    await page.getByTestId("mcp-call").click();
    await expect(page.getByTestId("mcp-result-notification")).toHaveCount(6);
    await page.getByTestId("mcp-about").click();
    await page.getByTestId("mcp-panel").evaluate((el) => {
      for (let n: Element | null = el; n; n = n.parentElement) n.scrollTop = 0;
    });
    await shot(page, "mcp-client-demo");
  });
});

/** Sets the assistant up through its settings dialog (a local model on this machine: the mock
 *  never opens a connection, and no real provider is involved). */
async function setUpAi(page: Page) {
  await command(page, "AI assistant: settings");
  await expect(page.getByTestId("ai-settings")).toBeVisible();
  await page.getByTestId("ai-provider").selectOption("openai-compatible");
  await page.getByTestId("ai-model").fill("llama3.1");
  await page.getByTestId("ai-base-url").fill("http://localhost:11434/v1");
  await page.getByTestId("ai-save").click();
  await expect(page.getByTestId("ai-settings")).toBeHidden();
}

async function sendHealth(page: Page) {
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");
}

test.describe("ai", () => {
  test("ki-einstellungen", async ({ page }) => {
    await open(page);
    await command(page, "AI assistant: settings");
    const dialog = page.getByTestId("ai-settings");
    await expect(dialog).toBeVisible();
    await page.getByTestId("ai-provider").selectOption("openai-compatible");
    await page.getByTestId("ai-model").fill("llama3.1");
    await page.getByTestId("ai-base-url").fill("http://localhost:11434/v1");
    await expect(page.getByTestId("ai-local-note")).toBeVisible();
    await page.getByTestId("ai-add-profile").click();
    await page.getByTestId("ai-provider").selectOption("anthropic");
    await page.getByTestId("ai-model").fill("claude-opus-5-5");
    await page.getByTestId("ai-key").fill("example-key-not-real");
    await expect(page.getByTestId("ai-roles")).toBeVisible();
    await page.getByTestId("ai-role-fast").selectOption("default");
    await shot(page, "ki-einstellungen");
  });

  test("ki-vorschau", async ({ page }) => {
    await open(page);
    await setUpAi(page);
    await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
    await page.evaluate(() => ((window as unknown as { __rt: { backend: { ai: { secrets: string[] } } } }).__rt.backend.ai.secrets = ["demo-password-123"]));
    await page.keyboard.press(`${MOD}+n`);
    await page.locator('[data-testid=new-tile][data-key="ai-request"]').click();
    await page.getByTestId("ai-create-folder").selectOption("users");
    await page.getByTestId("ai-create-description").fill("Create an order for a customer, login password demo-password-123");
    await page.getByTestId("ai-create-propose").click();
    await expect(page.getByTestId("ai-preview")).toBeVisible();
    await expect(page.getByTestId("ai-preview-masked")).toContainText("secret value");
    await shot(page, "ki-vorschau");
  });

  test("ki-vorschlag", async ({ page }) => {
    await open(page);
    await setUpAi(page);
    await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
    await page.keyboard.press(`${MOD}+n`);
    await page.locator('[data-testid=new-tile][data-key="ai-request"]').click();
    await page.getByTestId("ai-create-folder").selectOption("users");
    await page.getByTestId("ai-create-description").fill("Create an order");
    await page.getByTestId("ai-create-propose").click();
    await page.getByTestId("ai-preview-send").click();
    await expect(page.getByTestId("suggested-edit")).toBeVisible();
    await expect(page.getByTestId("suggest-block").first()).toBeVisible();
    await shot(page, "ki-vorschlag");
  });
});

// Plugins (mock backend): the first-start question, the permission question, the manager.

test.describe("plugins", () => {
  test("plugins-start", async ({ page }) => {
    await page.goto("/?backend=mock&welcome=1");
    const dialog = page.getByTestId("welcome-dialog");
    await expect(dialog).toBeVisible();
    await expect(dialog.getByRole("heading", { name: "What do you work with?" })).toBeVisible();
    await page.getByTestId("welcome-choice").check();
    await shot(page, "plugins-start");
  });

  const demo = (version: string) => ({
    id: "demo",
    name: "Request IDs",
    version,
    api: 1,
    description: "Adds an X-Request-ID header to every request and notes it in the response.",
    license: "Apache-2.0",
    homepage: null,
    permissions: { network: ["api.example.com"], storage: true, variables: false },
    contributes: { prepare: true, response: true, tabs: [{ id: "req", title: "Request ID", place: "request", content_types: [] }] },
  });

  async function pickPlugin(page: Page, version: string) {
    await page.getByTestId("plugins-file").setInputFiles({
      name: "request-ids.wasm",
      mimeType: "application/wasm",
      buffer: Buffer.from(JSON.stringify(demo(version))),
    });
  }

  test("plugins-rechte", async ({ page }) => {
    await open(page);
    await page.getByTestId("tree-request").filter({ hasText: "Create user" }).click();
    await command(page, "Manage plugins");
    await expect(page.getByTestId("plugins-dialog")).toBeVisible();
    await pickPlugin(page, "1.0.0");
    await expect(page.getByTestId("plugins-confirm")).toContainText("Install Request IDs 1.0.0?");
    await shot(page, "plugins-rechte");
  });

  test("plugins-verwalten", async ({ page }) => {
    await open(page);
    await page.getByTestId("tree-request").filter({ hasText: "Create user" }).click();
    await command(page, "Manage plugins");
    await pickPlugin(page, "1.0.0");
    await page.getByTestId("plugins-confirm-install").click();
    await expect(page.getByTestId("plugin-item")).toHaveCount(1);
    await shot(page, "plugins-verwalten");
  });
});

// OData (real core): the plugin runs in the browser's WebAssembly core against the local test
// service `rt-odata-testserver` (invented SAP-style data). A route adds the CORS headers the
// test service does not send, so the page may read its answers; nothing leaves the machine.

const ROOT = fileURLToPath(new URL("../../../../", import.meta.url));
const ODATA_PORT = { entities: 48911, query: 48912, systems: 48913 };
const ODATA_BIN = `${ROOT}target/debug/rt-odata-testserver`;
const ODATA_PLUGIN = `${ROOT}target/plugins/odata.wasm`;
const EXAMPLE = `${ROOT}examples/s4hana-sales`;

function exampleFiles(port: number): Record<string, string> {
  const out: Record<string, string> = {};
  const walk = (dir: string, prefix: string) => {
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      if (e.isDirectory()) walk(`${dir}/${e.name}`, `${prefix}${e.name}/`);
      else if (e.name.endsWith(".yml")) {
        out[`${prefix}${e.name}`] = readFileSync(`${dir}/${e.name}`, "utf8")
          .replace("http://127.0.0.1:47830", `http://127.0.0.1:${port}`)
          .replace("{{password}}", "RT_PASS");
      }
    }
  };
  walk(EXAMPLE, "");
  {
    out["metadata.yml"] = 'info:\n  name: Service metadata\n  type: http\n  seq: 9\n\nhttp:\n  method: GET\n  url: "{{baseUrl}}{{service}}/$metadata"\n  auth: inherit\n';
  }
  return out;
}

async function startOData(port: number): Promise<() => void> {
  const child = spawn(ODATA_BIN, ["--port", String(port)], { stdio: "ignore" });
  for (let i = 0; i < 100; i++) {
    try {
      await fetch(`http://127.0.0.1:${port}/`);
      break;
    } catch {
      await new Promise((r) => setTimeout(r, 100));
    }
  }
  return () => void child.kill();
}

test.describe("odata", () => {
  test.skip(!existsSync(ODATA_BIN) || !existsSync(ODATA_PLUGIN), "build rt-odata-testserver and run `node scripts/build-plugins.mjs` first");

  async function setupOData(page: Page, port: number) {
    const cors = {
      "access-control-allow-origin": "*",
      "access-control-allow-headers": "*",
      "access-control-allow-methods": "*",
      "access-control-expose-headers": "*",
    };
    await page.route(`http://127.0.0.1:${port}/**`, async (route) => {
      if (route.request().method() === "OPTIONS") return route.fulfill({ status: 204, headers: cors });
      const response = await route.fetch();
      await route.fulfill({ response, headers: { ...response.headers(), ...cors } });
    });
    await page.addInitScript(() => {
      Object.defineProperty(window, "showDirectoryPicker", {
        configurable: true,
        value: async () => (await navigator.storage.getDirectory()).getDirectoryHandle("collection", { create: true }),
      });
    });
    await page.goto("/");
    await page.evaluate(
      async ({ files }) => {
        const root = await (await navigator.storage.getDirectory()).getDirectoryHandle("collection", { create: true });
        for (const [path, text] of Object.entries(files)) {
          const parts = path.split("/");
          let dir = root;
          for (const part of parts.slice(0, -1)) dir = await dir.getDirectoryHandle(part, { create: true });
          const w = await (await dir.getFileHandle(parts[parts.length - 1]!, { create: true })).createWritable();
          await w.write(text);
          await w.close();
        }
      },
      { files: exampleFiles(port) },
    );
    await page.evaluate(async (bytes) => {
      const b = (window as unknown as { __rt: { backend: import("@requesttailor/ui").Backend } }).__rt.backend;
      await b.plugins.install({ name: "odata.wasm", data: new Uint8Array(bytes) });
    }, [...readFileSync(ODATA_PLUGIN)]);
    await page.getByTestId("open-collection").click();
    await expect(page.getByTestId("collection-name")).toContainText("S/4HANA Sales");
    await page.getByTestId("env-switcher").selectOption("Local");
  }

  test("odata-entitaeten", async ({ page }) => {
    const stop = await startOData(ODATA_PORT.entities);
    try {
      await setupOData(page, ODATA_PORT.entities);
      // Sending a request to <service root>$metadata is what teaches the plugin the model.
      await page.getByTestId("tree-request").filter({ hasText: "Service metadata" }).click();
      await page.getByTestId("send").click();
      await expect(page.getByTestId("response-status")).toHaveText(/200/);
      await page.getByTestId("collection-tabs").click();
      const tab = page.getByTestId("collection-panel").getByTestId("plugin-tab");
      await expect(tab).toContainText("SalesOrderSet");
      await shot(page, "odata-entitaeten");
    } finally {
      stop();
    }
  });

  test("odata-query", async ({ page }) => {
    const stop = await startOData(ODATA_PORT.query);
    try {
      await setupOData(page, ODATA_PORT.query);
      await page.getByTestId("tree-request").filter({ hasText: "Service metadata" }).click();
      await page.getByTestId("send").click();
      await expect(page.getByTestId("response-status")).toHaveText(/200/);
      await page.getByTestId("tree-request").filter({ hasText: "Read sales order with items" }).click();
      await page.getByTestId("tab-plugin-odata/query").click();
      await page.getByTestId("send").click();
      await expect(page.getByTestId("response-status")).toHaveText(/200/);
      await shot(page, "odata-query");
    } finally {
      stop();
    }
  });

  test("odata-systeme", async ({ page }) => {
    const stop = await startOData(ODATA_PORT.systems);
    try {
      await setupOData(page, ODATA_PORT.systems);
      await page.getByTestId("collection-tabs").click();
      await page.getByRole("tab", { name: "SAP systems" }).click();
      const form = page.getByTestId("collection-panel");
      await form.getByLabel("System name (e.g. S4H)").fill("S4H");
      await form.getByLabel("DEV host (e.g. s4d.example.corp:44300)").fill("s4d.example.corp:44300");
      await form.getByLabel("DEV client (e.g. 100)").fill("100");
      await form.getByLabel("QAS host (e.g. s4q.example.corp:44300)").fill("s4q.example.corp:44300");
      await form.getByLabel("QAS client (e.g. 100)").fill("200");
      await form.getByLabel("PRD host (e.g. s4p.example.corp:44300)").fill("s4p.example.corp:44300");
      await form.getByLabel("PRD client (e.g. 100)").fill("300");
      await form.getByLabel("Service path (optional, e.g. /sap/opu/odata/sap/API_PRODUCT_SRV)").fill("/sap/opu/odata/sap/ZRT_SALES_SRV");
      await shot(page, "odata-systeme", { x: 0, y: 0, width: 1440, height: 420 });
    } finally {
      stop();
    }
  });
});
