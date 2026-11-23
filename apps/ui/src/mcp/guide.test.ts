// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { DemoSession, demoWeather } from "../commands/mock-mcp-demo";
import { MCP_DEMO_URL } from "../commands/backend";
import { advance, doneCount, GUIDE_STEPS, isDemo, nextStep, noProgress } from "./guide";
import { notificationLine, serverTitle } from "./model";

describe("the guided first run", () => {
  it("goes connect, list, call, save, run, and only in that order", () => {
    let p = noProgress();
    expect(nextStep(p)).toBe("connect");
    expect(doneCount(p)).toBe(0);
    // A later step alone does not count.
    expect(advance(p, "call")).toBe(p);
    for (const s of GUIDE_STEPS) {
      expect(nextStep(p)).toBe(s.id);
      p = advance(p, s.id);
    }
    expect(nextStep(p)).toBeNull();
    expect(doneCount(p)).toBe(GUIDE_STEPS.length);
  });

  it("knows the built-in demo server", () => {
    expect(isDemo({ transport: "http", url: MCP_DEMO_URL, headers: [] })).toBe(true);
    expect(isDemo({ transport: "http", url: ` ${MCP_DEMO_URL}/ `, headers: [] })).toBe(true);
    expect(isDemo({ transport: "http", url: "http://localhost:3333/mcp", headers: [] })).toBe(false);
    expect(isDemo({ transport: "stdio", command: "rt", args: ["mcp-demo"], env: [], cwd: null })).toBe(false);
  });

  it("names the demo, the local demo and rt mcp in the panel header", () => {
    const local = "http://127.0.0.1:48409/mcp";
    expect(serverTitle({ transport: "http", url: MCP_DEMO_URL, headers: [] }, MCP_DEMO_URL, null)).toBe("Demo server (built in)");
    expect(serverTitle({ transport: "http", url: local, headers: [] }, MCP_DEMO_URL, local)).toBe("Demo server (local port)");
    expect(serverTitle({ transport: "stdio", command: "rt", args: ["mcp", "--agent-name", "x"], env: [], cwd: null }, MCP_DEMO_URL, null)).toBe(
      "RequestTailor itself (rt mcp)",
    );
  });
});

describe("notifications of a call", () => {
  it("shows progress with its share and logs with their level", () => {
    expect(notificationLine({ method: "notifications/progress", params: { progressToken: 1, progress: 1, total: 4, message: "step 1 of 4" } })).toEqual({
      kind: "progress",
      label: "progress",
      text: "step 1 of 4",
      progress: 0.25,
    });
    expect(notificationLine({ method: "notifications/progress", params: { progressToken: 1, progress: 7 } })).toMatchObject({ text: "7", progress: null });
    expect(notificationLine({ method: "notifications/message", params: { level: "warning", data: "careful" } })).toMatchObject({
      kind: "log",
      label: "warning",
      text: "careful",
    });
    expect(notificationLine({ method: "notifications/message", params: { level: "info", data: { a: 1 } } }).text).toBe('{"a":1}');
    expect(notificationLine({ method: "notifications/tools/list_changed", params: null }).label).toBe("tools/list_changed");
  });
});

describe("the mock demo server", () => {
  it("has the same weather as the Rust demo server", () => {
    // Pinned in crates/rt-app/src/mcp_demo/tests.rs.
    expect(demoWeather("Vienna")).toEqual({ city: "Vienna", temperature: 21, units: "metric", conditions: "sunny", humidity: 30, wind: 12 });
    expect(demoWeather("Vienna", "imperial").temperature).toBe(Math.round((21 * 9) / 5 + 32));
  });

  it("keeps to-dos per session and reports progress for countdown", () => {
    const a = new DemoSession();
    const b = new DemoSession();
    a.call("todo_add", { title: "Water the plants" });
    expect(a.call("todo_list", {})?.structured).toMatchObject({ open: 1 });
    expect(b.call("todo_list", {})?.structured).toMatchObject({ open: 0, items: [] });
    expect(a.call("todo_done", { id: 9 })?.is_error).toBe(true);

    const c = a.call("countdown", { from: 2 });
    expect(c?.structured).toEqual({ steps: 2 });
    expect(c?.notifications?.map((n) => n.method)).toEqual([
      "notifications/progress",
      "notifications/message",
      "notifications/progress",
      "notifications/message",
    ]);
    expect(a.call("fail", {})?.is_error).toBe(true);
    expect(a.call("nope", {})).toBeNull();
  });
});
