// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `AiApi` of the mock backend (wave 8, strand E): config and keys in memory, canned answers,
// the same refusals as the real shells (off until configured, Anthropic needs a key). No call
// leaves the page. `asked` lists the tasks, for UI tests.

import { BackendError, type AiApi } from "./backend";
import { withAiWave16, type AiWave8Api } from "./wave16-stubs";
import type { AiConfig, AiProviderKind, AiTask, RequestSpec } from "./types";

export interface MockAiApi extends AiApi {
  readonly asked: AiTask[];
}

function slug(text: string): string {
  return (
    text
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-|-$/g, "")
      .split("-")
      .slice(0, 3)
      .join("-") || "items"
  );
}

function proposal(description: string): RequestSpec {
  const create = /\b(create|add|new|post)\b/i.test(description);
  const name = description.trim().split(/\s+/).slice(0, 4).join(" ") || "New request";
  return {
    name: name.charAt(0).toUpperCase() + name.slice(1),
    seq: null,
    method: create ? "POST" : "GET",
    url: `{{baseUrl}}/${slug(description.replace(/\b(create|add|new|get|list|an?|the)\b/gi, ""))}`,
    headers: [{ name: "Accept", value: "application/json", enabled: true }],
    params: [],
    body: create ? { type: "raw", kind: "json", data: '{\n  "name": "{{name}}"\n}' } : { type: "none" },
    auth: { type: "inherit" },
    vars: [],
    settings: { timeout_ms: null, follow_redirects: null, max_redirects: null, encode_url: null },
    scripts: [],
    assertions: [],
    actions: [],
  };
}

export function mockAi(): MockAiApi {
  let config: AiConfig | null = null;
  const keys = new Map<AiProviderKind, string>();
  const asked: AiTask[] = [];
  // Wave 16: profiles, preview and generic calls arrive with strand A (placeholders until then).
  return withAiWave16<AiWave8Api & { asked: AiTask[] }>({
    asked,
    config: async () => (config ? { ...config } : null),
    setConfig: async (next) => {
      if (next && next.model.trim() === "") throw new BackendError("not-configured", "no AI provider is set up");
      if (next?.provider === "openai-compatible" && !next.base_url?.trim()) {
        throw new BackendError("not-configured", "no AI provider is set up");
      }
      config = next ? { ...next } : null;
    },
    hasKey: async (provider) => keys.has(provider),
    setKey: async (provider, key) => {
      if (key.trim() === "") keys.delete(provider);
      else keys.set(provider, key.trim());
    },
    deleteKey: async (provider) => void keys.delete(provider),
    ask: async (task) => {
      if (!config) throw new BackendError("not-configured", "no AI provider is set up");
      if (config.provider === "anthropic" && !keys.has("anthropic")) {
        throw new BackendError("no-key", "no API key for this provider");
      }
      asked.push(task);
      switch (task.task) {
        case "create-request":
          return { text: `A request for: *${task.description}*.`, request: proposal(task.description) };
        case "explain-error":
          return {
            text: "**The server refused the request.** The most likely cause is a missing or expired token.\n\n1. Check the `Authorization` header.\n2. Fetch a new token and send again.",
          };
        case "suggest-tests":
          return {
            text: "Checks the status and that the body is JSON.",
            scripts: [
              {
                phase: "tests",
                code: 'test("status is 200", () => {\n  expect(rt.response.status).to.equal(200);\n});',
              },
            ],
            assertions: [
              { expression: "res.status", operator: "eq", value: "200", description: "status is OK", enabled: true },
              { expression: "res.body", operator: "isJson", value: "", description: null, enabled: true },
            ],
          };
        default:
          // Wave 16 tasks: their canned answers arrive with strand B.
          throw new BackendError("unsupported", "This assistant task arrives in wave 16 (strand B).");
      }
    },
  });
}
