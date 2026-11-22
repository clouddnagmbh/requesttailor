// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `AiApi` of the web app (wave 8, strand E). Owner: wave 8, strand E.
//
// - Off until the user sets it up. The config (provider, model, base URL; no key) lives in
//   localStorage; without storage the assistant stays off.
// - Keys live in the encrypted browser secret store (`deps.openSecrets`, the same scheme as
//   secret values) under collection `requesttailor:ai`, environment `keys`, variable = the
//   provider id (`rt_app::ai::key_slot`). The UI can set and delete one and ask whether one
//   is set; it never gets one back.
// - `ask` hands the core (`Workspace.aiAsk`, crates/rt-wasm/src/ai_js.rs) the input as for a
//   send (secret values from the store, session variables) and the stored OAuth tokens, all
//   only for masking. The call goes out with `fetch`, or through `rt agent` when the
//   companion is on (e.g. a local model that does not allow this page's origin).

import { BackendError, type AiAnswer, type AiApi, type AiConfig, type AiProviderKind, type AiTask, type SecretsApi, type SendInput } from "@requesttailor/ui";

import { WebTokens } from "./auth";
import { toBackendError, type CompanionCredentials, type CoreSendInput, type CoreWorkspace } from "./wasm";
import type { Session } from "./web-backend";

/** `rt_app::ai::{KEY_COLLECTION, KEY_ENVIRONMENT}`. */
export const KEY_COLLECTION = "requesttailor:ai";
export const KEY_ENVIRONMENT = "keys";
export const CONFIG_STORAGE_KEY = "requesttailor.ai.config";

/** `Workspace.aiAsk` of the core (crates/rt-wasm/src/ai_js.rs). */
interface AiWorkspace extends CoreWorkspace {
  aiAsk(
    task: AiTask,
    input: CoreSendInput,
    config: AiConfig,
    key: string | null,
    extraSecrets: string[],
    companion: CompanionCredentials | null,
  ): Promise<AiAnswer>;
}

export interface AiHost {
  /** The open collection; throws `BackendError` if none. */
  session(): Session;
  openSecrets(collectionId: string): Promise<SecretsApi>;
  /** The core input of a send (secret values, session variables) and the companion, if on. */
  sendContext(input: SendInput): Promise<{ input: CoreSendInput; companion: CompanionCredentials | null }>;
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

const PROVIDERS: AiProviderKind[] = ["anthropic", "openai-compatible"];

function parseConfig(raw: string | null): AiConfig | null {
  if (!raw) return null;
  try {
    const c = JSON.parse(raw) as Partial<AiConfig>;
    if (!c || !PROVIDERS.includes(c.provider as AiProviderKind) || typeof c.model !== "string") return null;
    return { provider: c.provider as AiProviderKind, model: c.model, base_url: typeof c.base_url === "string" ? c.base_url : null };
  } catch {
    return null;
  }
}

/** `AiConfig::validate` (crates/rt-app/src/ai.rs). */
export function validateConfig(config: AiConfig): void {
  if (config.model.trim() === "") throw new BackendError("not-configured", "no AI provider is set up");
  const url = (config.base_url ?? "").trim();
  if (url === "") {
    if (config.provider === "openai-compatible") throw new BackendError("not-configured", "no AI provider is set up");
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
  const readConfig = (): AiConfig | null => {
    try {
      return parseConfig(storage()?.getItem(CONFIG_STORAGE_KEY) ?? null);
    } catch {
      return null;
    }
  };
  const readKey = async (provider: AiProviderKind): Promise<string | null> => {
    const key = await (await keyStore()).get(KEY_ENVIRONMENT, provider);
    return key && key.trim() !== "" ? key : null;
  };

  return {
    config: async () => readConfig(),
    setConfig: (config) =>
      guard(async () => {
        const s = storage();
        if (config === null) {
          s?.removeItem(CONFIG_STORAGE_KEY);
          return;
        }
        validateConfig(config);
        if (!s) throw new BackendError("shell", "This browser does not allow saving settings.");
        s.setItem(CONFIG_STORAGE_KEY, JSON.stringify(config));
      }),
    hasKey: (provider) => guard(async () => (await readKey(provider)) !== null),
    setKey: (provider, key) =>
      guard(async () => {
        const store = await keyStore();
        if (key.trim() === "") await store.delete(KEY_ENVIRONMENT, provider);
        else await store.set(KEY_ENVIRONMENT, provider, key.trim());
      }),
    deleteKey: (provider) => guard(async () => (await keyStore()).delete(KEY_ENVIRONMENT, provider)),
    ask: (task, input) =>
      guard(async () => {
        const config = readConfig();
        if (!config) throw new BackendError("not-configured", "no AI provider is set up");
        validateConfig(config);
        const key = await readKey(config.provider);
        if (config.provider === "anthropic" && key === null) {
          throw new BackendError("no-key", "no API key for this provider");
        }
        const s = host.session();
        const context = await host.sendContext({ environment: input.environment, vars: input.vars });
        const tokens = await tokenValues(await host.openSecrets(s.id));
        return (s.ws as AiWorkspace).aiAsk(task, context.input, config, key, tokens, context.companion);
      }),
  };
}
