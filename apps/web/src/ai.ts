// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `AiApi` of the web app (wave 8, strand E; wave 16, strand A).
//
// - Off until the user sets it up. The settings (profiles, roles, preview mode; no key) live in
//   localStorage; without storage the assistant stays off. A wave-8 config reads as the
//   profile `default`. Validation and the first-use bookkeeping are the core's
//   (`aiSettingsNormalize`, `aiSettingsConfirm`, crates/rt-wasm/src/ai_js.rs): the same code as
//   the desktop app.
// - Keys live in the encrypted browser secret store (`deps.openSecrets`) under collection
//   `requesttailor:ai`, environment `keys`; variable = the provider id, or `profile:<id>` for
//   a profile's own key (`rt_app::ai::{key_slot, profile_key_slot}`). A profile uses its own
//   key, else its provider's. The UI never gets one back.
// - Calls: the core builds the masked call (`Workspace.aiPreview`, `aiComplete`, `aiAsk`);
//   they go out with `fetch` where the provider allows a web page to call it
//   (`capabilities.browser_direct`), through `rt agent` where it does not and the companion
//   is on, and with `fetch` first for OpenAI-compatible endpoints (a local model). Otherwise
//   the call is refused with the reason, and nothing is sent.
// - A collection can be turned off on this device (a list of collection ids in localStorage);
//   the file's own switch is read by the core.

import {
  BackendError,
  providerInfo,
  type AiAnswer,
  type AiApi,
  type AiCollectionPolicy,
  type AiConfig,
  type AiEvent,
  type AiPreview,
  type AiProviderKind,
  type AiReply,
  type AiSettings,
  type SecretsApi,
  type SendInput,
} from "@requesttailor/ui";

import { WebTokens } from "./auth";
import { toBackendError, type CompanionCredentials, type CoreSendInput, type CoreWorkspace } from "./wasm";
import type { Session } from "./web-backend";

/** `rt_app::ai::{KEY_COLLECTION, KEY_ENVIRONMENT}`. */
export const KEY_COLLECTION = "requesttailor:ai";
export const KEY_ENVIRONMENT = "keys";
export const CONFIG_STORAGE_KEY = "requesttailor.ai.config";
export const OFF_STORAGE_KEY = "requesttailor.ai.off";

/** The AI part of `Workspace` (crates/rt-wasm/src/ai_js.rs). */
interface AiWorkspace extends CoreWorkspace {
  aiPolicy(deviceOff: boolean): AiCollectionPolicy;
  aiPreview(what: unknown, input: CoreSendInput, settings: AiSettings, extra: string[], deviceOff: boolean): AiPreview;
  aiAsk(
    task: unknown,
    input: CoreSendInput,
    settings: AiSettings,
    key: string | null,
    extra: string[],
    deviceOff: boolean,
    companion: CompanionCredentials | null,
  ): Promise<AiAnswer>;
  aiComplete(
    request: unknown,
    input: CoreSendInput,
    settings: AiSettings,
    key: string | null,
    extra: string[],
    deviceOff: boolean,
    companion: CompanionCredentials | null,
    onEvent: (event: AiEvent) => void,
  ): Promise<AiReply>;
}

/** The free functions of the core module the settings use. */
export interface AiCore {
  aiSettingsNormalize(next: AiSettings, old: AiSettings | null): AiSettings;
  aiSettingsConfirm(settings: AiSettings, profile: string): AiSettings;
  aiCancel(id: string): void;
}

export interface AiHost {
  /** The open collection; throws `BackendError` if none. */
  session(): Session;
  openSecrets(collectionId: string): Promise<SecretsApi>;
  /** The core input of a send (secret values, session variables) and the companion, if on. */
  sendContext(input: SendInput): Promise<{ input: CoreSendInput; companion: CompanionCredentials | null }>;
  /** The loaded core module (its AI functions). */
  core(): Promise<AiCore>;
  /** `localStorage`, or `null` where there is none. */
  storage?: () => Storage | null;
}

function browserStorage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

const KINDS = ["anthropic", "openai", "azure-openai", "gemini", "mistral", "openai-compatible", "bedrock"];
const LABELS = Object.fromEntries(KINDS.map((k) => [k, providerInfo(k as AiProviderKind).label]));

/** `rt_app::ai::AiSettings::parse`: settings, or a wave-8 config as the profile `default`. */
export function parseSettings(raw: string | null): AiSettings | null {
  if (!raw) return null;
  try {
    const v = JSON.parse(raw) as Record<string, unknown> | null;
    if (!v || typeof v !== "object") return null;
    if (Array.isArray(v.profiles)) {
      const s = v as unknown as AiSettings;
      return { ...s, roles: s.roles ?? {}, preview: s.preview ?? "first-use" };
    }
    if (typeof v.provider === "string" && KINDS.includes(v.provider) && typeof v.model === "string") {
      const config: AiConfig = {
        provider: v.provider as AiProviderKind,
        model: v.model,
        base_url: typeof v.base_url === "string" ? v.base_url : null,
      };
      return { profiles: [{ id: "default", label: LABELS[config.provider] ?? config.provider, ...config }], roles: {}, preview: "first-use" };
    }
  } catch {
    // Unreadable settings count as "not set up".
  }
  return null;
}

/** `AiSettings::resolve(None)`. */
function defaultProfile(s: AiSettings) {
  return s.profiles.find((p) => p.id === s.default_profile) ?? s.profiles[0] ?? null;
}

/** `AiConfig::validate` (crates/rt-app/src/ai.rs). */
export function validateConfig(config: AiConfig): void {
  if (config.model.trim() === "") throw new BackendError("not-configured", "no AI provider is set up");
  const url = (config.base_url ?? "").trim();
  if (url === "") {
    if (providerInfo(config.provider).capabilities.needs_base_url) {
      throw new BackendError("not-configured", "no AI provider is set up");
    }
    return;
  }
  if (!url.startsWith("http://") && !url.startsWith("https://")) {
    throw new BackendError("transport", `the provider could not be reached: the base URL must start with http:// or https://: ${url}`);
  }
}

/** Values of the collection's stored OAuth tokens, for masking only. */
async function tokenValues(store: SecretsApi): Promise<string[]> {
  const tokens = new WebTokens(store);
  const out: string[] = [];
  for (const info of await tokens.list()) {
    const t = await tokens.get(info.key);
    for (const v of [t?.access_token, t?.refresh_token, t?.id_token]) if (v) out.push(v);
  }
  return out;
}

async function guard<T>(f: () => Promise<T>): Promise<T> {
  try {
    return await f();
  } catch (e) {
    throw toBackendError(e);
  }
}

const ROLE_OF_TASK: Record<string, "fast" | "smart"> = {
  "suggest-names": "fast",
  "explain-response": "fast",
  "draft-docs": "fast",
};

export function webAi(host: AiHost): AiApi {
  const storage = host.storage ?? browserStorage;
  let keys: Promise<SecretsApi> | null = null;
  const keyStore = () => {
    keys ??= host.openSecrets(KEY_COLLECTION).catch((e: unknown) => {
      keys = null;
      throw e;
    });
    return keys;
  };
  const read = (): AiSettings | null => {
    try {
      return parseSettings(storage()?.getItem(CONFIG_STORAGE_KEY) ?? null);
    } catch {
      return null;
    }
  };
  const write = (s: AiSettings | null) => {
    const st = storage();
    if (s === null) {
      st?.removeItem(CONFIG_STORAGE_KEY);
      return;
    }
    if (!st) throw new BackendError("shell", "This browser does not allow saving settings.");
    st.setItem(CONFIG_STORAGE_KEY, JSON.stringify(s));
  };
  const offIds = (): string[] => {
    try {
      const v = JSON.parse(storage()?.getItem(OFF_STORAGE_KEY) ?? "[]") as unknown;
      return Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : [];
    } catch {
      return [];
    }
  };
  const deviceOff = (id: string) => offIds().includes(id);
  const slotKey = async (variable: string): Promise<string | null> => {
    const key = await (await keyStore()).get(KEY_ENVIRONMENT, variable);
    return key && key.trim() !== "" ? key : null;
  };
  const setSlot = async (variable: string, key: string) => {
    const store = await keyStore();
    if (key.trim() === "") await store.delete(KEY_ENVIRONMENT, variable);
    else await store.set(KEY_ENVIRONMENT, variable, key.trim());
  };

  /** Settings, the profile a call will use, its key, and the core input; throws like the
   *  desktop app does (`not-configured`, `no-key`). */
  const setup = async (wanted: string | undefined, input: SendInput, send: boolean) => {
    const settings = read();
    if (!settings || settings.profiles.length === 0) throw new BackendError("not-configured", "no AI provider is set up");
    const profile =
      wanted === "fast" ? settings.profiles.find((p) => p.id === settings.roles.fast) ?? defaultProfile(settings)
      : wanted === "smart" ? settings.profiles.find((p) => p.id === settings.roles.smart) ?? defaultProfile(settings)
      : wanted ? settings.profiles.find((p) => p.id === wanted) ?? null
      : defaultProfile(settings);
    if (!profile) throw new BackendError("not-configured", "no AI provider is set up");
    const key = (await slotKey(`profile:${profile.id}`)) ?? (await slotKey(profile.provider));
    const caps = providerInfo(profile.provider).capabilities;
    if (send && profile.provider !== "openai-compatible" && key === null) throw new BackendError("no-key", "no API key for this provider");
    const s = host.session();
    const context = await host.sendContext({ environment: input.environment, vars: input.vars });
    const tokens = await tokenValues(await host.openSecrets(s.id));
    // `fetch` where the provider allows it; else the companion; a local model is tried
    // directly first.
    let companion: CompanionCredentials | null = null;
    if (!caps.browser_direct && profile.provider !== "openai-compatible") {
      if (!context.companion && send) {
        throw new BackendError(
          "transport",
          `${providerInfo(profile.provider).label} does not allow calls from a web page. Start the RequestTailor companion (\`rt agent\`) and connect it, then try again.`,
        );
      }
      companion = context.companion;
    }
    return { settings, profile, key, ws: s.ws as AiWorkspace, input: context.input, tokens, companion, id: s.id };
  };

  return {
    config: async () => {
      const s = read();
      const p = s ? defaultProfile(s) : null;
      return p ? { provider: p.provider, model: p.model, base_url: p.base_url ?? null } : null;
    },
    setConfig: (config) =>
      guard(async () => {
        if (config === null) return write(null);
        validateConfig(config);
        const old = read();
        const p = old ? defaultProfile(old) : null;
        let next: AiSettings;
        if (old && p) {
          next = { ...old, profiles: old.profiles.map((q) => (q.id === p.id ? { ...q, ...config } : q)) };
        } else {
          next = { profiles: [{ id: "default", label: LABELS[config.provider] ?? config.provider, ...config }], roles: {}, preview: "first-use" };
        }
        write((await host.core()).aiSettingsNormalize(next, old));
      }),
    hasKey: (provider) => guard(async () => (await slotKey(provider)) !== null),
    setKey: (provider, key) => guard(() => setSlot(provider, key)),
    deleteKey: (provider) => guard(async () => (await keyStore()).delete(KEY_ENVIRONMENT, provider)),
    settings: async () => read(),
    setSettings: (next) =>
      guard(async () => {
        if (next === null) return write(null);
        write((await host.core()).aiSettingsNormalize(next, read()));
      }),
    hasProfileKey: (profile) => guard(async () => (await slotKey(`profile:${profile}`)) !== null),
    setProfileKey: (profile, key) => guard(() => setSlot(`profile:${profile}`, key)),
    deleteProfileKey: (profile) => guard(async () => (await keyStore()).delete(KEY_ENVIRONMENT, `profile:${profile}`)),
    policy: () =>
      guard(async () => {
        const s = host.session();
        return (s.ws as AiWorkspace).aiPolicy(deviceOff(s.id));
      }),
    setDeviceOptOut: (off) =>
      guard(async () => {
        const id = host.session().id;
        const ids = offIds().filter((i) => i !== id);
        if (off) ids.push(id);
        const st = storage();
        if (!st) throw new BackendError("shell", "This browser does not allow saving settings.");
        st.setItem(OFF_STORAGE_KEY, JSON.stringify(ids));
      }),
    preview: (what, input) =>
      guard(async () => {
        const wanted = what.kind === "request" ? what.request.profile : ROLE_OF_TASK[what.task.task] ?? "smart";
        const c = await setup(wanted, input, false);
        return c.ws.aiPreview(what, c.input, c.settings, c.tokens, deviceOff(c.id));
      }),
    confirmPreview: (profile) =>
      guard(async () => {
        const s = read();
        if (!s) throw new BackendError("not-configured", "no AI provider is set up");
        write((await host.core()).aiSettingsConfirm(s, profile));
      }),
    ask: (task, input) =>
      guard(async () => {
        const c = await setup(ROLE_OF_TASK[task.task] ?? "smart", input, true);
        return c.ws.aiAsk(task, c.input, c.settings, c.key, c.tokens, deviceOff(c.id), c.companion);
      }),
    complete: (request, input, onEvent) =>
      guard(async () => {
        const c = await setup(request.profile, input, true);
        return c.ws.aiComplete(request, c.input, c.settings, c.key, c.tokens, deviceOff(c.id), c.companion, (e) => onEvent?.(e));
      }),
    cancel: async (id) => {
      (await host.core()).aiCancel(id);
    },
  };
}
