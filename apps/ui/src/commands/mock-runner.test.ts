// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { AppState } from "../state/app.svelte";
import { RunnerState } from "../runner/runner.svelte";
import { MockBackend } from "./mock";
import { parseDataMock } from "./mock-runner";
import type { RunEvent, RunPlan } from "./types";

const enc = (s: string) => new TextEncoder().encode(s);

async function open(): Promise<MockBackend> {
  const b = new MockBackend();
  await b.openCollection();
  return b;
}

const plan = (requests: string[], extra: Partial<RunPlan> = {}): RunPlan => ({
  requests,
  data: [],
  iterations: 1,
  delay_ms: 0,
  bail: false,
  ...extra,
});

describe("mock runner", () => {
  it("plans in tree order, per folder", async () => {
    const b = await open();
    expect(await b.runner.plan(null)).toEqual([
      "demo/blocked.yml",
      "demo/large.yml",
      "health.yml",
      "users/create-user.yml",
      "users/list-users.yml",
    ]);
    expect(await b.runner.plan("users")).toEqual(["users/create-user.yml", "users/list-users.yml"]);
    expect(await b.runner.plan("nope")).toEqual([]);
  });

  it("runs iterations with data rows as variables and reports steps", async () => {
    const b = await open();
    const path = await b.createRequest("", "Echo");
    const spec = await b.getRequest(path);
    await b.updateRequest(path, {
      ...spec,
      url: "{{baseUrl}}/echo?user={{user}}",
      assertions: [
        { expression: "res.status", operator: "eq", value: "200", description: null, enabled: true },
        { expression: "res.status", operator: "eq", value: "201", description: null, enabled: true },
      ],
    });
    const events: RunEvent[] = [];
    const data = await b.runner.parseData({ name: "u.csv", data: enc("user\nada\nbob\n") });
    const summary = await b.runner.run(plan([path, "health.yml"], { data }), { environment: "Local", vars: [] }, (e) =>
      events.push(e),
    );
    expect(events[0]).toEqual({ kind: "started", total: 4 });
    const steps = events.flatMap((e) => (e.kind === "step" ? [e.step] : []));
    expect(steps.map((s) => [s.iteration, s.request_path])).toEqual([
      [0, path],
      [0, "health.yml"],
      [1, path],
      [1, "health.yml"],
    ]);
    expect(steps[0]!.report.request?.url).toBe("http://127.0.0.1:47811/echo?user=ada");
    expect(steps[2]!.report.request?.url).toBe("http://127.0.0.1:47811/echo?user=bob");
    // One assertion fails: the step fails, the health check passes.
    expect(steps[0]!.passed).toBe(false);
    expect(steps[1]!.passed).toBe(true);
    expect(summary).toMatchObject({
      requests: 4,
      passed: 2,
      failed: 2,
      tests_passed: 4,
      assertions_passed: 2,
      assertions_failed: 2,
      stopped: false,
    });
    expect(events.at(-1)).toEqual({ kind: "finished", summary });
  });

  it("bails, cancels and runs one at a time", async () => {
    const b = await open();
    const all = await b.runner.plan(null);
    const bailed = await b.runner.run(plan(all, { bail: true }), { environment: "Local", vars: [] }, () => {});
    // demo/blocked.yml comes first and fails (blocked by the browser).
    expect(bailed).toMatchObject({ requests: 1, failed: 1, stopped: true });

    const events: RunEvent[] = [];
    const running = b.runner.run(plan(all, { iterations: 50, delay_ms: 20 }), { environment: "Local", vars: [] }, (e) => {
      events.push(e);
    });
    await expect(b.runner.run(plan(all), { environment: "Local", vars: [] }, () => {})).rejects.toThrow(/in progress/);
    await new Promise((r) => setTimeout(r, 60));
    await b.runner.cancel();
    const summary = await running;
    expect(summary.stopped).toBe(true);
    expect(summary.requests).toBeLessThan(250);
    // Big bodies arrive as a preview.
    const large = events.flatMap((e) => (e.kind === "step" && e.step.request_path === "demo/large.yml" ? [e.step] : []));
    expect(large[0]?.report.response?.body.data.length).toBeLessThanOrEqual(4096);
    expect(large[0]?.report.response?.truncated).toBe(true);
  });

  it("parses CSV and JSON data like the core", () => {
    expect(parseDataMock("a.csv", enc('﻿a,b\r\n1,"x, ""y"""\r\n'))).toEqual([
      [
        ["a", "1"],
        ["b", 'x, "y"'],
      ],
    ]);
    expect(parseDataMock("a.json", enc('[{"id": 1, "ok": true}]'))).toEqual([
      [
        ["id", 1],
        ["ok", true],
      ],
    ]);
    expect(() => parseDataMock("a.csv", enc("a,b\n1\n"))).toThrow(/line 2/);
    expect(() => parseDataMock("a.json", enc('{"a": 1}'))).toThrow(/array/);
    expect(() => parseDataMock("a.txt", enc("x"))).toThrow(/\.csv or \.json/);
  });
});

describe("RunnerState", () => {
  it("runs the open target with the active environment and keeps the results", async () => {
    const b = await open();
    const app = new AppState(b);
    await app.open();
    const runner = new RunnerState(app);
    runner.open({ folder: "users", name: "users" });
    runner.iterations = 2;
    await runner.start();
    expect(runner.running).toBe(false);
    expect(runner.results.steps).toHaveLength(4);
    expect(runner.results.summary).toMatchObject({ requests: 4, passed: 4 });
    expect(runner.results.steps[0]!.report.environment).toBe("Local");
    expect(runner.ran).toEqual({ folder: "users", name: "users" });

    await runner.loadData({ name: "bad.csv", data: enc("a,b\n1\n") });
    expect(runner.dataError).toMatch(/line 2/);
    expect(runner.data).toBeNull();
    await runner.loadData({ name: "ok.json", data: enc('[{"a": 1}, {"a": 2}, {"a": 3}]') });
    expect(runner.dataError).toBeNull();
    expect(runner.data).toHaveLength(3);
    await runner.start();
    expect(runner.results.steps).toHaveLength(6);
    runner.clearData();
    expect(runner.dataName).toBeNull();
  });
});
