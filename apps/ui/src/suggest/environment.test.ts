// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `SuggestedEdit.environment` (wave 17): variables added to an environment, secrets without a
// value, unsaved, one undo step, also inside a batch; `dismissKey` and the dialog outcome.

import { describe, expect, it } from "vitest";

import { MockBackend } from "../commands/mock";
import type { SuggestOutcome } from "../extensions";
import { AppState } from "../state/app.svelte";
import { prepare } from "./apply";
import { createSuggestOutcome } from "./suggest";

async function setup() {
  const backend = new MockBackend();
  const app = new AppState(backend);
  await app.open();
  const env = (await backend.environments())[0]!;
  return { backend, app, env };
}

describe("environment suggestions", () => {
  it("adds variables, declares a secret without a value and undoes in one step", async () => {
    const { backend, app, env } = await setup();
    const p = await prepare(backend, app, {
      kind: "environment",
      environment: env.name,
      variables: [
        { name: "page_size", value: "50" },
        { name: "api_token", value: "RT-CANARY-123", secret: true },
        { name: env.variables[0]!.name, value: "ignored" },
      ],
    });
    expect(p.blocks.map((b) => b.label)).toEqual(["Variable › page_size", "Variable › api_token"]);
    expect(JSON.stringify(p.blocks)).not.toContain("RT-CANARY");
    const applied = await p.apply();
    const now = (await backend.environments()).find((e) => e.name === env.name)!;
    expect(now.variables.find((v) => v.name === "page_size")?.value).toBe("50");
    expect(now.variables.find((v) => v.name === "api_token")).toMatchObject({ secret: true, value: null });
    expect(now.variables.find((v) => v.name === env.variables[0]!.name)).toEqual(env.variables[0]);
    expect(await applied.undo!()).toBe(true);
    expect((await backend.environments()).find((e) => e.name === env.name)!.variables).toEqual(env.variables);
  });

  it("refuses an unknown environment and a changed one", async () => {
    const { backend, app, env } = await setup();
    await expect(prepare(backend, app, { kind: "environment", environment: "nope", variables: [] })).rejects.toMatchObject({ kind: "not-found" });
    const p = await prepare(backend, app, { kind: "environment", environment: env.name, variables: [{ name: "x", value: "1" }] });
    await backend.updateEnvironment({ ...env, variables: [...env.variables, { name: "y", value: "2", enabled: true, secret: false }] });
    await expect(p.apply()).rejects.toMatchObject({ kind: "stale" });
  });

  it("is a part of a batch", async () => {
    const { backend, app, env } = await setup();
    const p = await prepare(backend, app, {
      kind: "batch",
      edits: [
        { kind: "environment", environment: env.name, variables: [{ name: "token", secret: true }] },
        { kind: "request", path: "users/list-users.yml", spec: { ...(await backend.getRequest("users/list-users.yml")), url: "https://x.test/{{token}}" } },
      ],
    });
    const applied = await p.apply();
    expect((await backend.environments()).find((e) => e.name === env.name)!.variables.some((v) => v.name === "token")).toBe(true);
    expect(await applied.undo!()).toBe(true);
  });
});

describe("dismissing a suggestion", () => {
  async function outcome(close: unknown, dismissKey?: string): Promise<{ outcome: SuggestOutcome; offered: boolean }> {
    const { backend, app, env } = await setup();
    const run = createSuggestOutcome(backend, app)({ kind: "environment", environment: env.name, variables: [{ name: "z", value: "1" }], dismissKey });
    await new Promise((r) => setTimeout(r, 20));
    const prepared = app.dialog!.dialog.props!.prepared as { dismissKey?: string };
    const offered = prepared.dismissKey !== undefined;
    app.closeDialog(close);
    return { outcome: await run, offered };
  }

  it("reports dismissed only when the dialog was closed with it", async () => {
    expect(await outcome("dismissed", "k1")).toEqual({ outcome: { applied: false, dismissed: true }, offered: true });
    expect(await outcome(true, "k1")).toEqual({ outcome: { applied: true }, offered: true });
    expect(await outcome(undefined)).toEqual({ outcome: { applied: false }, offered: false });
  });
});
