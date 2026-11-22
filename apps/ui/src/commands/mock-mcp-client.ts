// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `McpClientApi` in the mock backend (wave 8, strand D): one demo MCP server, good enough for
// UI tests. It answers over "HTTP" at any `http(s)://` URL except hosts named `unreachable`,
// and over "stdio" for the command `demo-mcp` (anything else: command not found). Tools:
// `echo` (text), `add` (structured output), `fail` (isError) and `image`. `{{variables}}`
// in the server config and the arguments come from the selected environment and the runtime
// variables (`host.lookup`). Saved tests are staged like request edits: `dirty()` lists
// them and `save()` writes them (`MockBackend` includes both). The real client and file
// format live in Rust (rt_app::mcp_client, rt_format::mcp_test) and are tested there.

import { BackendError, type McpClientApi, type SendInput } from "./backend";
import { mockEvaluate } from "./mock-assert";
import type { FileChange, McpCallResult, McpServerConfig, McpTest, McpTestResult, McpTool } from "./types";

export interface MockMcpHost {
  /** Value of a variable for `input` (environment, then runtime), `undefined` if unknown. */
  lookup(input: SendInput, name: string): string | undefined;
}

export interface MockMcp extends McpClientApi {
  /** Staged test paths that differ from the saved ones. */
  dirty(): string[];
  /** Writes the staged tests (mock rendering). */
  save(): FileChange[];
}

const PIXEL =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

export const MOCK_MCP_TOOLS: McpTool[] = [
  {
    name: "echo",
    title: "Echo",
    description: "Returns the text it was given.",
    input_schema: {
      type: "object",
      properties: { text: { type: "string", description: "What to echo" } },
      required: ["text"],
    },
    output_schema: null,
    annotations: { readOnlyHint: true },
  },
  {
    name: "add",
    title: null,
    description: "Adds two numbers.",
    input_schema: {
      type: "object",
      properties: { a: { type: "number" }, b: { type: "number" } },
      required: ["a", "b"],
    },
    output_schema: { type: "object", properties: { sum: { type: "number" } }, required: ["sum"] },
    annotations: null,
  },
  {
    name: "fail",
    title: null,
    description: "Always reports a tool error.",
    input_schema: { type: "object", properties: {} },
    output_schema: null,
    annotations: { destructiveHint: false },
  },
  {
    name: "image",
    title: null,
    description: "Returns a 1x1 PNG.",
    input_schema: { type: "object" },
    output_schema: null,
    annotations: null,
  },
];

const resolveWith = (lookup: (name: string) => string | undefined) => (text: string) =>
  text.replace(/\{\{\s*([^{}\s]+)\s*\}\}/g, (all, name: string) => lookup(name) ?? all);

function resolveValue(value: unknown, resolve: (s: string) => string): unknown {
  if (typeof value === "string") return resolve(value);
  if (Array.isArray(value)) return value.map((v) => resolveValue(v, resolve));
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, resolveValue(v, resolve)]));
  }
  return value;
}

function connectCheck(server: McpServerConfig, resolve: (s: string) => string): void {
  if (server.transport === "stdio") {
    const command = resolve(server.command);
    if (command !== "demo-mcp") throw new BackendError("shell", `${command}: command not found`);
    return;
  }
  const url = resolve(server.url);
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    throw new BackendError("shell", `cannot connect to ${url}: not a URL`);
  }
  if (!parsed.protocol.startsWith("http")) throw new BackendError("shell", `cannot connect to ${url}: not HTTP`);
  if (parsed.hostname === "unreachable") {
    throw new BackendError("shell", `cannot connect to ${url}: connection refused`);
  }
}

function callTool(tool: string, args: unknown): McpCallResult {
  const a = (args ?? {}) as Record<string, unknown>;
  const result = (content: McpCallResult["content"], structured: unknown = null, isError = false): McpCallResult => ({
    content,
    structured,
    is_error: isError,
    time_ms: 3,
  });
  switch (tool) {
    case "echo":
      return result([{ type: "text", text: String(a.text ?? "") }]);
    case "add": {
      const sum = Number(a.a) + Number(a.b);
      return result([{ type: "text", text: JSON.stringify({ sum }) }], { sum });
    }
    case "fail":
      return result([{ type: "text", text: "This tool always fails." }], null, true);
    case "image":
      return result([{ type: "image", mime_type: "image/png", data: PIXEL }]);
    default:
      throw new BackendError("shell", `MCP error -32602: Unknown tool: ${tool}`);
  }
}

/** The body assertions see: the structured content, else the first text (as rt_app does). */
function testResponse(r: McpCallResult): { status: number; body: string } {
  const text = r.content.find((c) => c.type === "text");
  const body = r.structured !== null ? JSON.stringify(r.structured) : text && text.type === "text" ? text.text : "";
  return { status: r.is_error ? 500 : 200, body };
}

function slug(name: string): string {
  const s = name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return s === "" ? "test" : s;
}

export function mockMcp(host: MockMcpHost): MockMcp {
  const open = new Map<number, { resolve: (s: string) => string }>();
  let nextId = 1;
  const staged = new Map<string, McpTest>();
  const saved = new Map<string, McpTest>();
  const resolver = (input: SendInput) => resolveWith((name) => host.lookup(input, name));

  const connection = (id: number) => {
    const c = open.get(id);
    if (!c) throw new BackendError("shell", `no MCP connection ${id}`);
    return c;
  };

  const testOf = (path: string): McpTest => {
    const t = staged.get(path) ?? saved.get(path);
    if (!t) throw new BackendError("request-not-found", path);
    return structuredClone(t);
  };

  return {
    async connect(server, input) {
      const resolve = resolver(input);
      connectCheck(server, resolve);
      const id = nextId++;
      open.set(id, { resolve });
      return {
        id,
        info: {
          name: "demo-mcp",
          version: "1.0.0",
          protocol_version: "2025-11-25",
          capabilities: ["tools"],
          instructions: "A demo server of the mock backend.",
        },
      };
    },
    async tools(id) {
      connection(id);
      return structuredClone(MOCK_MCP_TOOLS);
    },
    async call(id, tool, args) {
      const { resolve } = connection(id);
      return callTool(tool, resolveValue(args, resolve));
    },
    async disconnect(id) {
      open.delete(id);
    },
    async tests() {
      return [...new Set([...saved.keys(), ...staged.keys()])].sort();
    },
    async getTest(path) {
      return testOf(path);
    },
    async saveTest(path, test) {
      if (test.name.trim() === "") throw new BackendError("invalid-input", "A test needs a name.");
      let target = path;
      if (target === null) {
        const base = `mcp/${slug(test.name)}`;
        target = `${base}.mcp.yaml`;
        for (let n = 2; staged.has(target) || saved.has(target); n++) target = `${base}-${n}.mcp.yaml`;
      }
      staged.set(target, structuredClone(test));
      return target;
    },
    async runTest(path, input): Promise<McpTestResult> {
      const test = testOf(path);
      const resolve = resolver(input);
      try {
        connectCheck(test.server, resolve);
        const result = callTool(test.tool, resolveValue(test.arguments, resolve));
        const lookup = (name: string) => host.lookup(input, name);
        const enabled = test.assertions.filter((a) => a.enabled);
        const assertions = mockEvaluate(enabled, testResponse(result), lookup);
        // As rt_app: without assertions a test passes when the call is not an error.
        const passed = assertions.length === 0 ? !result.is_error : assertions.every((a) => a.passed);
        return { path, passed, result, error: null, assertions };
      } catch (e) {
        return { path, passed: false, result: null, error: e instanceof Error ? e.message : String(e), assertions: [] };
      }
    },
    dirty() {
      return [...staged.keys()].filter((p) => JSON.stringify(staged.get(p)) !== JSON.stringify(saved.get(p))).sort();
    },
    save() {
      const changes: FileChange[] = [];
      for (const path of this.dirty()) {
        const test = staged.get(path)!;
        saved.set(path, test);
        changes.push({ path, text: `# mock rendering, not *.mcp.yaml\n${JSON.stringify(test, null, 2)}\n`, deleted: false });
      }
      staged.clear();
      return changes;
    },
  };
}
