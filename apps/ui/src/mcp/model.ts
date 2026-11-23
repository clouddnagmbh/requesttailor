// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Pure helpers of the MCP client view (wave 8, strand D): server drafts, tool annotations,
// suggested assertions for "Save as test".

import type { Assertion, McpCallResult, McpNotification, McpServerConfig, McpTool } from "../commands/types";

/** An editable server config: both transports' fields, so switching keeps what was typed. */
export interface ServerDraft {
  transport: "stdio" | "http";
  command: string;
  /** One argument per line. */
  args: string;
  env: [string, string][];
  cwd: string;
  url: string;
  headers: [string, string][];
}

export function emptyDraft(): ServerDraft {
  return { transport: "http", command: "", args: "", env: [], cwd: "", url: "", headers: [] };
}

const filled = (rows: [string, string][]) => rows.filter(([n]) => n.trim() !== "").map(([n, v]): [string, string] => [n.trim(), v]);

export function configOf(d: ServerDraft): McpServerConfig {
  if (d.transport === "http") return { transport: "http", url: d.url.trim(), headers: filled(d.headers) };
  return {
    transport: "stdio",
    command: d.command.trim(),
    args: d.args.split("\n").map((a) => a.trim()).filter((a) => a !== ""),
    env: filled(d.env),
    cwd: d.cwd.trim() === "" ? null : d.cwd.trim(),
  };
}

export function draftOf(c: McpServerConfig): ServerDraft {
  const d = emptyDraft();
  if (c.transport === "http") return { ...d, transport: "http", url: c.url, headers: c.headers.map(([n, v]) => [n, v]) };
  return {
    ...d,
    transport: "stdio",
    command: c.command,
    args: c.args.join("\n"),
    env: c.env.map(([n, v]) => [n, v]),
    cwd: c.cwd ?? "",
  };
}

/** Why the draft cannot connect yet, or `null`. */
export function draftProblem(d: ServerDraft): string | null {
  if (d.transport === "http") return d.url.trim() === "" ? "Enter the server URL." : null;
  return d.command.trim() === "" ? "Enter the command that starts the server." : null;
}

/** A short label of a server: the URL or the command line. */
export function serverLabel(c: McpServerConfig): string {
  return c.transport === "http" ? c.url : [c.command, ...c.args].join(" ");
}

const HINTS: [string, string][] = [
  ["readOnlyHint", "read-only"],
  ["destructiveHint", "destructive"],
  ["idempotentHint", "idempotent"],
  ["openWorldHint", "open world"],
];

/** The tool's annotations as short labels (`true` hints only, plus a title if different). */
export function annotationLabels(tool: McpTool): string[] {
  const a = tool.annotations;
  if (!a || typeof a !== "object") return [];
  const record = a as Record<string, unknown>;
  return HINTS.filter(([key]) => record[key] === true).map(([, label]) => label);
}

export function toolLabel(tool: McpTool): string {
  const title = tool.title ?? (tool.annotations as { title?: unknown } | null)?.title;
  return typeof title === "string" && title !== "" && title !== tool.name ? `${title} (${tool.name})` : tool.name;
}

function assertion(expression: string, operator: string, value: string): Assertion {
  return { expression, operator, value, description: null, enabled: true };
}

/** Assertions to start a saved test with, from the result it was saved from: the status
 *  (200, or 500 for a tool error) and, for structured output, each top-level scalar. */
export function suggestedAssertions(result: McpCallResult | null): Assertion[] {
  if (!result) return [assertion("res.status", "eq", "200")];
  const out = [assertion("res.status", "eq", result.is_error ? "500" : "200")];
  const s = result.structured;
  if (s && typeof s === "object" && !Array.isArray(s)) {
    for (const [key, value] of Object.entries(s).slice(0, 5)) {
      if (!/^[A-Za-z_$][\w$]*$/.test(key)) continue;
      if (typeof value === "number" || typeof value === "boolean") out.push(assertion(`res.body.${key}`, "eq", String(value)));
      else if (typeof value === "string") out.push(assertion(`res.body.${key}`, "eq", JSON.stringify(value)));
    }
  }
  return out;
}

/** Pretty JSON. */
export function pretty(value: unknown): string {
  return JSON.stringify(value, null, 2);
}

/** A test name from the tool and the arguments' first value. */
export function suggestedName(tool: string, args: unknown): string {
  const first = args && typeof args === "object" ? Object.values(args)[0] : undefined;
  const hint = typeof first === "string" || typeof first === "number" ? ` ${String(first).slice(0, 30)}` : "";
  return `${tool}${hint}`;
}

/** "Learn more" in the MCP view: the guide in the CLI README (opens only on click). */
export const LEARN_MORE_URL = "https://github.com/clouddnagmbh/requesttailor/blob/main/crates/rt-cli/README.md#mcp-testing-servers-and-the-demo-server";

/** What the MCP panel calls a server: the demo by name, else its URL or command line. */
export function serverTitle(c: McpServerConfig, demoUrl: string, localDemoUrl: string | null): string {
  if (c.transport === "http" && c.url.trim().replace(/\/$/, "") === demoUrl) return "Demo server (built in)";
  if (c.transport === "http" && localDemoUrl !== null && c.url.trim() === localDemoUrl) return "Demo server (local port)";
  if (c.transport === "stdio" && c.args[0] === "mcp" && /(^|[/\\])rt(\.exe)?$/.test(c.command)) return "RequestTailor itself (rt mcp)";
  return serverLabel(c);
}

/** A notification of a call as one line: progress with its share, a log with its level. */
export interface NotificationLine {
  kind: "progress" | "log" | "other";
  label: string;
  text: string;
  /** 0…1 for progress with a total, else `null`. */
  progress: number | null;
}

export function notificationLine(n: McpNotification): NotificationLine {
  const p = (n.params && typeof n.params === "object" ? n.params : {}) as Record<string, unknown>;
  if (n.method === "notifications/progress") {
    const done = typeof p.progress === "number" ? p.progress : 0;
    const total = typeof p.total === "number" && p.total > 0 ? p.total : null;
    const counts = total === null ? String(done) : `${done}/${total}`;
    const text = typeof p.message === "string" && p.message !== "" ? p.message : counts;
    return { kind: "progress", label: "progress", text, progress: total === null ? null : Math.min(1, done / total) };
  }
  if (n.method === "notifications/message") {
    const level = typeof p.level === "string" ? p.level : "info";
    const data = typeof p.data === "string" ? p.data : JSON.stringify(p.data ?? null);
    return { kind: "log", label: level, text: data, progress: null };
  }
  return { kind: "other", label: n.method.replace(/^notifications\//, ""), text: JSON.stringify(n.params), progress: null };
}
