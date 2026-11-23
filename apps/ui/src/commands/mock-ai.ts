// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `AiApi` of the mock backend (wave 8, strand E): config and keys in memory, canned answers,
// the same refusals as the real shells (off until configured, Anthropic needs a key). No call
// leaves the page. `asked` lists the tasks, for UI tests.

import { BackendError, type AiApi } from "./backend";
import { looksLikeCurl } from "../ai/task";
import { providerInfo } from "../ai/providers";
import type {
  AiCollectionPolicy,
  AiConfig,
  AiEvent,
  AiPreview,
  AiProfile,
  AiProviderKind,
  AiReply,
  AiSettings,
  AiTask,
  RequestSpec,
} from "./types";

export interface MockAiApi extends AiApi {
  readonly asked: AiTask[];
  /** What `complete` was asked, for UI tests (the masked calls, as previewed). */
  readonly sent: AiPreview["call"][];
  /** `extensions.requesttailor.ai.enabled: false` of the open collection (a test switch). */
  fileOff: boolean;
  /** Values masked as `***` in previews, as the core masks secret values. */
  secrets: string[];
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

/** The request a `curl` command describes (method, URL, `-H` headers, `-d` body), as the core's
 *  importer reads the common cases. */
function fromCurl(command: string): RequestSpec {
  const words = [...command.replace(/^\s*\$?\s*curl\s*/, "").matchAll(/"([^"]*)"|'([^']*)'|(\S+)/g)].map((m) => m[1] ?? m[2] ?? m[3]!);
  let method = "";
  let url = "";
  const headers: RequestSpec["headers"] = [];
  let data = "";
  for (let i = 0; i < words.length; i++) {
    const w = words[i]!;
    if (w === "-X" || w === "--request") method = (words[++i] ?? "").toUpperCase();
    else if (w === "-H" || w === "--header") {
      const [name, ...rest] = (words[++i] ?? "").split(":");
      if (name) headers.push({ name: name.trim(), value: rest.join(":").trim(), enabled: true });
    } else if (w === "-d" || w === "--data" || w === "--data-raw") data = words[++i] ?? "";
    else if (!w.startsWith("-") && url === "") url = w;
  }
  const json = data.trim().startsWith("{");
  const spec = proposal(url);
  return {
    ...spec,
    name: `${method || (data ? "POST" : "GET")} ${url.replace(/^https?:\/\//, "")}`,
    method: method || (data ? "POST" : "GET"),
    url,
    headers,
    body: data ? { type: "raw", kind: json ? "json" : "text", data } : { type: "none" },
    auth: { type: "inherit" },
  };
}

const ROLE_OF_TASK: Record<string, "fast" | "smart"> = { "suggest-names": "fast", "explain-response": "fast", "draft-docs": "fast" };

export function mockAi(): MockAiApi {
  let settings: AiSettings | null = null;
  const provider = new Set<AiProviderKind>();
  const own = new Set<string>();
  const asked: AiTask[] = [];
  const sent: AiPreview["call"][] = [];
  let deviceOff = false;
  const cancelled = new Set<string>();
  const notSetUp = () => new BackendError("not-configured", "no AI provider is set up");

  const defaultProfile = (s: AiSettings | null): AiProfile | null =>
    s ? (s.profiles.find((p) => p.id === s.default_profile) ?? s.profiles[0] ?? null) : null;
  const profileFor = (wanted?: string | null): AiProfile => {
    const s = settings;
    if (!s || s.profiles.length === 0) throw notSetUp();
    const by = (id?: string) => s.profiles.find((p) => p.id === id);
    const found =
      wanted === "fast" ? (by(s.roles.fast) ?? defaultProfile(s))
      : wanted === "smart" ? (by(s.roles.smart) ?? defaultProfile(s))
      : wanted ? by(wanted)
      : defaultProfile(s);
    if (!found) throw notSetUp();
    return found;
  };
  const keyFor = (p: AiProfile) => own.has(p.id) || provider.has(p.provider);
  const needsKey = (p: AiProfile) => p.provider !== "openai-compatible";

  const api: MockAiApi = {
    asked,
    sent,
    fileOff: false,
    secrets: [],
    config: async () => {
      const p = defaultProfile(settings);
      return p ? { provider: p.provider, model: p.model, base_url: p.base_url } : null;
    },
    setConfig: async (next) => {
      if (next === null) {
        settings = null;
        return;
      }
      check(next);
      const p = defaultProfile(settings);
      settings = p
        ? { ...settings!, profiles: settings!.profiles.map((q) => (q.id === p.id ? { ...q, ...next } : q)) }
        : { profiles: [{ id: "default", label: providerInfo(next.provider).label, ...next }], roles: {}, preview: "first-use" };
    },
    hasKey: async (p) => provider.has(p),
    setKey: async (p, key) => void (key.trim() === "" ? provider.delete(p) : provider.add(p)),
    deleteKey: async (p) => void provider.delete(p),
    settings: async () => (settings ? structuredClone(settings) : null),
    setSettings: async (next) => {
      if (next === null) {
        settings = null;
        return;
      }
      const ids = new Set<string>();
      for (const p of next.profiles) {
        if (!/^[a-z0-9-]{1,32}$/.test(p.id) || ids.has(p.id)) {
          throw new BackendError("transport", `a profile id must be 1 to 32 characters of a-z, 0-9 and -: ${p.id}`);
        }
        ids.add(p.id);
        check(p);
      }
      for (const r of [next.default_profile, next.roles.fast, next.roles.smart]) {
        if (r && !ids.has(r)) throw new BackendError("transport", "the default profile and the roles must name existing profiles");
      }
      // A profile that moved is a first use again (`AiSettings::carry_previewed`).
      const kept = (settings?.previewed ?? []).filter((id) => {
        const a = settings?.profiles.find((p) => p.id === id);
        const b = next.profiles.find((p) => p.id === id);
        return a && b && a.provider === b.provider && a.model === b.model && (a.base_url ?? null) === (b.base_url ?? null);
      });
      settings = { ...structuredClone(next), previewed: kept };
    },
    hasProfileKey: async (id) => own.has(id),
    setProfileKey: async (id, key) => void (key.trim() === "" ? own.delete(id) : own.add(id)),
    deleteProfileKey: async (id) => void own.delete(id),
    policy: async (): Promise<AiCollectionPolicy> => ({ file_off: api.fileOff, device_off: deviceOff }),
    setDeviceOptOut: async (off) => void (deviceOff = off),
    preview: async (what) => {
      const p = profileFor(what.kind === "request" ? what.request.profile : (ROLE_OF_TASK[what.task.task] ?? "smart"));
      if (api.fileOff || deviceOff) throw new BackendError("opted-out", "the AI assistant is off for this collection");
      const raw =
        what.kind === "request"
          ? { system: what.request.system, messages: what.request.messages, max_tokens: what.request.max_tokens ?? 4096, tools: what.request.tools ?? [] }
          : {
              system: `Task: ${what.task.task}`,
              messages: [{ role: "user" as const, content: JSON.stringify(what.task) }],
              max_tokens: 4096,
              tools: [],
            };
      let text = JSON.stringify(raw);
      let masked = 0;
      for (const secret of api.secrets) {
        if (secret === "") continue;
        const parts = text.split(secret);
        masked += parts.length - 1;
        text = parts.join("***");
      }
      const call = JSON.parse(text) as AiPreview["call"];
      const must = settings!.preview === "always" || (settings!.preview === "first-use" && !(settings!.previewed ?? []).includes(p.id));
      return {
        profile: p.id,
        provider: p.provider,
        model: p.model,
        endpoint: p.base_url ? `${p.base_url.replace(/\/$/, "")}/chat/completions` : `https://mock.invalid/${p.provider}`,
        call,
        masked,
        bytes: text.length,
        required: must,
      };
    },
    confirmPreview: async (id) => {
      if (!settings) throw notSetUp();
      if (!(settings.previewed ?? []).includes(id)) settings = { ...settings, previewed: [...(settings.previewed ?? []), id] };
    },
    complete: async (request, input, onEvent): Promise<AiReply> => {
      const p = profileFor(request.profile);
      if (api.fileOff || deviceOff) throw new BackendError("opted-out", "the AI assistant is off for this collection");
      if (needsKey(p) && !keyFor(p)) throw new BackendError("no-key", "no API key for this provider");
      const shown = await api.preview({ kind: "request", request }, input);
      sent.push(shown.call);
      const text = `Mock answer to: ${request.purpose}`;
      const emit = (e: AiEvent) => onEvent?.(e);
      for (const word of text.split(/(?<= )/)) {
        await new Promise((r) => setTimeout(r, 0));
        if (cancelled.delete(request.id)) {
          const error = { kind: "cancelled" as const, message: "cancelled" };
          emit({ kind: "error", error: error as never });
          throw new BackendError("cancelled", "cancelled");
        }
        emit({ kind: "delta", delta: { kind: "text", text: word } });
      }
      const reply: AiReply = { text, stop: "end" };
      emit({ kind: "done", reply });
      return reply;
    },
    cancel: async (id) => void cancelled.add(id),
    ask: async (task) => {
      // A cURL snippet is read without a model: no settings, no key, no opt-out needed.
      if (task.task === "request-from-snippet" && looksLikeCurl(task.snippet)) {
        asked.push(task);
        return { text: "Read from the cURL command, without a model.", request: fromCurl(task.snippet) };
      }
      const p = profileFor(ROLE_OF_TASK[task.task] ?? null);
      if (needsKey(p) && !keyFor(p)) throw new BackendError("no-key", "no API key for this provider");
      if (api.fileOff || deviceOff) throw new BackendError("opted-out", "the AI assistant is off for this collection");
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
        case "request-from-snippet":
          return { text: `A request from the snippet.`, request: proposal(task.snippet) };
        case "explain-response":
          return { text: "Returns **3 items** as a JSON array; each has an `id` and a `status`." };
        case "suggest-assertions":
          return {
            text: "Checks the status, the type of the body and a field of the first item.",
            assertions: [
              { expression: "res.status", operator: "eq", value: "200", description: "status is OK", enabled: true },
              { expression: "res.body.items", operator: "isArray", value: "", description: null, enabled: true },
            ],
          };
        case "suggest-names":
          return {
            text: "",
            names:
              task.target === "variable"
                ? ["baseUrl", "itemId"]
                : task.target === "folder"
                  ? ["Orders", "Order management"]
                  : ["List open orders", "Open orders", "Get open orders"],
          };
        case "draft-docs":
          return { text: "", docs: "Lists the **open orders**.\n\n- `status`: filters by order status." };
      }
    },
  };

  function check(c: AiConfig) {
    if (c.model.trim() === "") throw notSetUp();
    if (providerInfo(c.provider).capabilities.needs_base_url && !c.base_url?.trim()) throw notSetUp();
    if (c.base_url?.trim() && !/^https?:\/\//.test(c.base_url.trim())) {
      throw new BackendError("transport", `the provider could not be reached: the base URL must start with http:// or https://: ${c.base_url}`);
    }
  }
  return api;
}
