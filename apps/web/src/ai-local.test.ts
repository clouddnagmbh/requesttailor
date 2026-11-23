// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 16, strand B: a cURL snippet is answered by the core's importer (`aiLocalAnswer`), so the
// task works without settings, without a key and without a call. Everything else still goes the
// usual way and is refused while the assistant is not set up.

import { describe, expect, it } from "vitest";
import type { AiAnswer, AiTask } from "@requesttailor/ui";

import { webAi, type AiHost } from "./ai";
import type { Session } from "./web-backend";

const curl: AiTask = { task: "request-from-snippet", snippet: "curl https://api.test/a", folder: "" };
const description: AiTask = { task: "create-request", description: "list orders", folder: "" };
const input = { environment: null, vars: [] as [string, string][] };

function setup() {
  const asked: AiTask[] = [];
  const ws = { aiAsk: async (task: AiTask) => (asked.push(task), { text: "from the model" }) };
  const host: AiHost = {
    session: () => ({ id: "c1", ws }) as unknown as Session,
    openSecrets: async () => ({ has: async () => false, get: async () => null, set: async () => {}, delete: async () => {} }),
    sendContext: async (i) => ({ input: { ...i, secrets: [] }, companion: null }),
    core: async () => ({
      aiSettingsNormalize: (next) => next,
      aiSettingsConfirm: (s) => s,
      aiCancel: () => {},
      // Stands in for `ai::local_answer`: only a cURL snippet is answered.
      aiLocalAnswer: (task): AiAnswer | undefined =>
        task.task === "request-from-snippet" && task.snippet.startsWith("curl") ? { text: "Read from the cURL command, without a model." } : undefined,
    }),
    storage: () => null,
  };
  return { ai: webAi(host), asked };
}

describe("webAi.ask with a cURL snippet", () => {
  it("answers without settings and never reaches the model", async () => {
    const { ai, asked } = setup();
    expect((await ai.ask(curl, input)).text).toContain("without a model");
    expect(asked).toEqual([]);
  });

  it("still refuses a model task while the assistant is not set up", async () => {
    const { ai, asked } = setup();
    await expect(ai.ask(description, input)).rejects.toMatchObject({ kind: "not-configured" });
    expect(asked).toEqual([]);
  });
});
