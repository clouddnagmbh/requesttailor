// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// State of the MCP client view (wave 8, strand D): one connection at a time, its tools, the
// selected tool's arguments, the last result, and the saved tests (`*.mcp.yaml`) with their
// last run. Everything goes through `Backend.mcp`; values come back masked.

import { BackendError, type SendInput } from "../commands/backend";
import type { Assertion, McpCallResult, McpServerInfo, McpTestResult, McpTool } from "../commands/types";
import type { AppState } from "../state/app.svelte";
import { configOf, draftOf, draftProblem, emptyDraft, serverLabel, suggestedAssertions, suggestedName, type ServerDraft } from "./model";
import { fieldsOf, initialArgs } from "./schema";

export interface SaveDraft {
  name: string;
  assertions: Assertion[];
}

export class McpState {
  /** Whether the client view is shown in the main area. */
  open = $state(false);
  draft = $state<ServerDraft>(emptyDraft());
  connection = $state<{ id: number; info: McpServerInfo } | null>(null);
  connecting = $state(false);
  tools = $state<McpTool[]>([]);
  selected = $state<string | null>(null);
  args = $state<Record<string, unknown>>({});
  /** Raw JSON editing instead of the form. */
  raw = $state(false);
  rawText = $state("{}");
  calling = $state(false);
  result = $state<McpCallResult | null>(null);
  error = $state<string | null>(null);
  /** The saved test being edited (opened from the list), or `null`: "Save as test" makes a new one. */
  testPath = $state<string | null>(null);
  /** The "Save as test" form, while open. */
  saving = $state<SaveDraft | null>(null);
  saveError = $state<string | null>(null);
  tests = $state<string[]>([]);
  testResults = $state<Record<string, McpTestResult>>({});
  running = $state<Record<string, boolean>>({});
  testsError = $state<string | null>(null);
  /** stdio tests of this collection may start commands: asked once per collection and session. */
  stdioAllowed = $state<Record<string, boolean>>({});

  constructor(private readonly app: AppState) {}

  private get api() {
    return this.app.backend.mcp;
  }

  input(): SendInput {
    return { environment: this.app.environment, vars: [] };
  }

  get tool(): McpTool | null {
    return this.tools.find((t) => t.name === this.selected) ?? null;
  }

  show(): void {
    this.open = true;
  }

  close(): void {
    this.open = false;
  }

  get problem(): string | null {
    return draftProblem(this.draft);
  }

  async connect(): Promise<void> {
    if (this.problem) {
      this.error = this.problem;
      return;
    }
    await this.disconnect();
    this.connecting = true;
    this.error = null;
    try {
      const connection = await this.api.connect(configOf(this.draft), this.input());
      this.connection = connection;
      this.tools = await this.api.tools(connection.id);
      const keep = this.selected && this.tools.some((t) => t.name === this.selected) ? this.selected : null;
      this.select(keep ?? this.tools[0]?.name ?? null, keep !== null);
    } catch (e) {
      this.error = message(e);
    } finally {
      this.connecting = false;
    }
  }

  async disconnect(): Promise<void> {
    const c = this.connection;
    this.connection = null;
    this.tools = [];
    if (c) await this.api.disconnect(c.id).catch(() => {});
  }

  /** Selects a tool; `keepArgs` keeps the arguments typed so far (reconnect, opened test). */
  select(name: string | null, keepArgs = false): void {
    this.selected = name;
    this.result = null;
    if (keepArgs) return;
    const fields = this.tool ? fieldsOf(this.tool.input_schema) : null;
    this.args = fields ? initialArgs(fields) : {};
    this.rawText = JSON.stringify(this.args, null, 2);
    this.raw = fields === null;
  }

  setArgs(args: Record<string, unknown>): void {
    this.args = args;
    this.rawText = JSON.stringify(args, null, 2);
  }

  /** Raw JSON typed: the arguments when it is an object. */
  setRaw(text: string, parsed: Record<string, unknown> | null): void {
    this.rawText = text;
    if (parsed) this.args = parsed;
  }

  async call(): Promise<void> {
    const c = this.connection;
    if (!c || !this.selected) return;
    this.calling = true;
    this.error = null;
    try {
      this.result = await this.api.call(c.id, this.selected, $state.snapshot(this.args));
    } catch (e) {
      this.result = null;
      this.error = message(e);
    } finally {
      this.calling = false;
    }
  }

  // ---- saved tests ----

  startSave(): void {
    this.saveError = null;
    if (this.testPath) {
      // Editing a saved test: keep its name and assertions.
      void this.api.getTest(this.testPath).then(
        (t) => (this.saving = { name: t.name, assertions: t.assertions }),
        () => (this.saving = { name: suggestedName(this.selected ?? "test", this.args), assertions: suggestedAssertions(this.result) }),
      );
      return;
    }
    this.saving = { name: suggestedName(this.selected ?? "test", this.args), assertions: suggestedAssertions(this.result) };
  }

  /** Stages the test (written by Save, like request edits). `asNew` saves a copy. */
  async save(asNew = false): Promise<string | null> {
    const s = this.saving;
    if (!s || !this.selected) return null;
    if (s.name.trim() === "") {
      this.saveError = "Give the test a name.";
      return null;
    }
    try {
      const path = await this.api.saveTest(asNew ? null : this.testPath, {
        name: s.name.trim(),
        server: configOf(this.draft),
        tool: this.selected,
        arguments: $state.snapshot(this.args),
        assertions: $state.snapshot(s.assertions),
      });
      this.testPath = path;
      this.saving = null;
      this.app.dirty = await this.app.backend.dirty();
      await this.loadTests();
      return path;
    } catch (e) {
      this.saveError = message(e);
      return null;
    }
  }

  async loadTests(): Promise<void> {
    try {
      this.tests = await this.api.tests();
      this.testsError = null;
    } catch (e) {
      this.tests = [];
      this.testsError = e instanceof BackendError && e.kind === "no-collection" ? null : message(e);
    }
  }

  /** Opens a saved test in the view: its server (not connected yet), tool and arguments. */
  async openTest(path: string): Promise<void> {
    try {
      const t = await this.api.getTest(path);
      await this.disconnect();
      this.draft = draftOf(t.server);
      this.testPath = path;
      this.selected = t.tool;
      this.setArgs(t.arguments && typeof t.arguments === "object" && !Array.isArray(t.arguments) ? (t.arguments as Record<string, unknown>) : {});
      this.raw = false;
      this.result = null;
      this.saving = null;
      this.error = null;
      this.open = true;
    } catch (e) {
      this.testsError = message(e);
    }
  }

  newConnection(): void {
    void this.disconnect();
    this.testPath = null;
    this.saving = null;
    this.result = null;
    this.error = null;
    this.selected = null;
    this.open = true;
  }

  /** Commands of stdio tests, by path, that need the user's go-ahead before they run. */
  async stdioCommands(paths: string[]): Promise<string[]> {
    const id = this.app.collection?.id ?? "";
    if (this.stdioAllowed[id]) return [];
    const commands: string[] = [];
    for (const p of paths) {
      const t = await this.api.getTest(p).catch(() => null);
      if (t?.server.transport === "stdio") commands.push(`${p}: ${serverLabel(t.server)}`);
    }
    return commands;
  }

  allowStdio(): void {
    this.stdioAllowed[this.app.collection?.id ?? ""] = true;
  }

  async runTest(path: string): Promise<void> {
    this.running[path] = true;
    try {
      this.testResults[path] = await this.api.runTest(path, this.input());
    } catch (e) {
      this.testResults[path] = { path, passed: false, result: null, error: message(e), assertions: [] };
    } finally {
      this.running[path] = false;
    }
  }

  async runAll(): Promise<void> {
    for (const path of this.tests) await this.runTest(path);
  }

  get counts(): { passed: number; failed: number } {
    const results = this.tests.map((p) => this.testResults[p]).filter((r) => r !== undefined);
    return { passed: results.filter((r) => r.passed).length, failed: results.filter((r) => !r.passed).length };
  }

  /** A new collection: nothing of the old one stays. */
  reset(): void {
    void this.disconnect();
    this.tests = [];
    this.testResults = {};
    this.testPath = null;
    this.saving = null;
    this.result = null;
    this.error = null;
  }
}

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
