// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `AuthApi` of the web app (wave 5, strand C): tokens in the encrypted browser store, the
// authorization code flow in a popup that returns to the app's origin, device code shown in
// the UI. Token requests during a send go through the send's transport (browser or
// companion); "Get token" fetches from the browser.
//
// - Tokens: the secret store of the collection (`deps.openSecrets`, encrypted), environment
//   `oauth2`, variable = the core's cache key, plus an `index` entry listing the keys (as
//   `rt_app::auth::SecretTokens` does natively). The UI only ever gets `TokenInfo`.
// - Browser step: the callback URL must be `<this origin>/oauth-callback.html` (registered
//   at the provider). That page (public/oauth-callback.html) posts its URL on the
//   BroadcastChannel `requesttailor-oauth` and closes; this side waits for it. The popup
//   may be blocked: the prompt dialog also shows the link.

import {
  BackendError,
  type AuthApi,
  type AuthPrompt,
  type DeviceCodePrompt,
  type SecretsApi,
  type SendInput,
  type TokenInfo,
} from "@requesttailor/ui";

import type { CoreSendServices } from "./send-services";
import type { CoreSendInput, CoreWorkspace } from "./wasm";
import type { WebHost } from "./web-backend";

/** `AuthApi` plus the auth part of a send (the `auth` field `services_js` reads). */
export interface WebAuthApi extends AuthApi {
  forSend(collectionId: string, environment: string | null): Promise<unknown>;
}

/** Mirrors `rt_core::services::StoredToken`. */
export interface StoredToken {
  access_token: string;
  refresh_token: string | null;
  id_token: string | null;
  token_type: string | null;
  scope: string | null;
  expires_at: number | null;
  obtained_at: number;
}

/** The `auth` field of the services object (crates/rt-wasm/src/auth_js.rs). */
export interface CoreAuthCallbacks {
  get(key: string): Promise<StoredToken | null>;
  put(key: string, token: StoredToken): Promise<void>;
  remove(key: string): Promise<void>;
  authorize(url: string, redirectUri: string): Promise<string>;
  deviceCode(prompt: DeviceCodePrompt): void;
  deviceCodeDone(): void;
  cancelled(): boolean;
}

/** `Workspace.fetchToken` of rt-wasm (wave 5, strand C). */
type TokenWorkspace = CoreWorkspace & {
  fetchToken(
    path: string,
    input: CoreSendInput,
    companion: null,
    services: CoreSendServices,
  ): Promise<TokenInfo>;
};

export const TOKEN_ENVIRONMENT = "oauth2";
const INDEX = "index";
export const CALLBACK_PATH = "/oauth-callback.html";
export const CHANNEL = "requesttailor-oauth";
/** How long the browser step may take. */
const TIMEOUT_MS = 5 * 60 * 1000;

/** Display label of a cache key (`rt_core::auth::oauth2::key_label`): host · client · scope. */
export function keyLabel(key: string): string {
  const parts = key.split("|").map((p) => p.replaceAll("%7C", "|").replaceAll("%25", "%"));
  if (parts.length !== 8 || parts[0] !== "oauth2" || parts[1] !== "1") return key;
  const [, , , endpoint, client, scope, id, extra] = parts as [string, string, string, string, string, string, string, string];
  let host = endpoint;
  try {
    host = new URL(endpoint).hostname || endpoint;
  } catch {
    // keep the raw value
  }
  const label = [host, client, scope, id];
  if (extra.startsWith("user=")) label.push(extra.slice("user=".length));
  return label.filter((s) => s !== "").join(" · ");
}

/** The tokens of one collection in its encrypted secret store. */
export class WebTokens {
  constructor(private readonly store: SecretsApi) {}

  private async index(): Promise<string[]> {
    const raw = await this.store.get(TOKEN_ENVIRONMENT, INDEX);
    try {
      const keys: unknown = raw ? JSON.parse(raw) : [];
      return Array.isArray(keys) ? keys.filter((k): k is string => typeof k === "string") : [];
    } catch {
      return [];
    }
  }

  private async writeIndex(keys: string[]): Promise<void> {
    if (keys.length === 0) await this.store.delete(TOKEN_ENVIRONMENT, INDEX);
    else await this.store.set(TOKEN_ENVIRONMENT, INDEX, JSON.stringify(keys));
  }

  async get(key: string): Promise<StoredToken | null> {
    const raw = await this.store.get(TOKEN_ENVIRONMENT, key);
    if (!raw) return null;
    try {
      return JSON.parse(raw) as StoredToken;
    } catch {
      return null;
    }
  }

  async put(key: string, token: StoredToken): Promise<void> {
    await this.store.set(TOKEN_ENVIRONMENT, key, JSON.stringify(token));
    const keys = await this.index();
    if (!keys.includes(key)) await this.writeIndex([...keys, key]);
  }

  async remove(key: string): Promise<void> {
    await this.store.delete(TOKEN_ENVIRONMENT, key);
    const keys = await this.index();
    if (keys.includes(key)) await this.writeIndex(keys.filter((k) => k !== key));
  }

  async list(): Promise<TokenInfo[]> {
    const out: TokenInfo[] = [];
    for (const key of await this.index()) {
      const t = await this.get(key);
      if (!t) continue;
      out.push({
        key,
        label: keyLabel(key),
        obtained_at: t.obtained_at,
        expires_at: t.expires_at,
        refreshable: t.refresh_token != null,
      });
    }
    return out.sort((a, b) => a.obtained_at - b.obtained_at);
  }

  async clear(key: string | null): Promise<void> {
    if (key != null) return this.remove(key);
    for (const k of await this.index()) await this.store.delete(TOKEN_ENVIRONMENT, k);
    await this.writeIndex([]);
  }
}

/** Browser parts, injectable for tests. */
export interface AuthEnv {
  origin: string;
  open(url: string): { close(): void } | null;
  channel(name: string): {
    onmessage: ((e: MessageEvent) => void) | null;
    close(): void;
  };
}

const BROWSER_ENV: AuthEnv = {
  get origin() {
    return window.location.origin;
  },
  open: (url) => window.open(url, "requesttailor-oauth", "popup,width=520,height=720"),
  channel: (name) => new BroadcastChannel(name),
};

export function webAuth(host: WebHost, env: AuthEnv = BROWSER_ENV): WebAuthApi {
  const handlers = new Set<(prompt: AuthPrompt) => void>();
  const emit = (prompt: AuthPrompt) => {
    for (const h of handlers) {
      try {
        h(prompt);
      } catch (e) {
        console.error(e);
      }
    }
  };
  const stores = new Map<string, Promise<WebTokens>>();
  const tokens = (collectionId: string): Promise<WebTokens> => {
    let t = stores.get(collectionId);
    if (!t) {
      t = host.deps.openSecrets(collectionId).then((s) => new WebTokens(s));
      t.catch(() => stores.delete(collectionId));
      stores.set(collectionId, t);
    }
    return t;
  };

  let cancelled = false;
  let abort: ((e: Error) => void) | null = null;

  const authorize = (url: string, redirectUri: string): Promise<string> => {
    let redirect: URL;
    try {
      redirect = new URL(redirectUri);
    } catch {
      return Promise.reject(new Error(`the callback URL ${JSON.stringify(redirectUri)} is not a URL`));
    }
    if (redirect.origin !== env.origin || redirect.pathname !== CALLBACK_PATH) {
      return Promise.reject(
        new Error(
          `in the browser the callback URL must be ${env.origin}${CALLBACK_PATH}; ` +
            "register it at the provider and use it as callback URL",
        ),
      );
    }
    if (cancelled) return Promise.reject(new Error("cancelled"));
    return new Promise<string>((resolve, reject) => {
      const channel = env.channel(CHANNEL);
      let popup: { close(): void } | null = null;
      const finish = (f: () => void) => {
        clearTimeout(timer);
        channel.onmessage = null;
        channel.close();
        abort = null;
        try {
          popup?.close();
        } catch {
          // the provider's page may forbid it
        }
        emit({ kind: "done" });
        f();
      };
      const timer = setTimeout(
        () => finish(() => reject(new Error("the sign-in in the browser took too long"))),
        TIMEOUT_MS,
      );
      abort = (e) => finish(() => reject(e));
      channel.onmessage = (e: MessageEvent) => {
        const data = e.data as { url?: unknown } | null;
        if (typeof data?.url !== "string") return;
        let back: URL;
        try {
          back = new URL(data.url);
        } catch {
          return;
        }
        if (back.origin !== redirect.origin || back.pathname !== redirect.pathname) return;
        const returned = data.url;
        finish(() => resolve(returned));
      };
      emit({ kind: "browser", url });
      popup = env.open(url);
    });
  };

  const forSend = async (collectionId: string): Promise<CoreAuthCallbacks> => {
    const store = await tokens(collectionId);
    cancelled = false;
    return {
      get: (key) => store.get(key),
      put: (key, token) => store.put(key, token),
      remove: (key) => store.remove(key),
      authorize,
      deviceCode: (prompt) => emit({ kind: "device-code", ...prompt }),
      deviceCodeDone: () => emit({ kind: "done" }),
      cancelled: () => cancelled,
    };
  };

  /** Values of the enabled secret variables of `environment` (as at send). */
  const secretValues = async (
    collectionId: string,
    ws: CoreWorkspace,
    environment: string | null,
  ): Promise<[string, string][]> => {
    const e = environment == null ? undefined : ws.environments().find((x) => x.name === environment);
    const names = e?.variables.filter((v) => v.secret && v.enabled).map((v) => v.name) ?? [];
    if (!e || names.length === 0) return [];
    const store = await host.deps.openSecrets(collectionId);
    const values: [string, string][] = [];
    for (const name of names) {
      const value = await store.get(e.name, name);
      if (value != null) values.push([name, value]);
    }
    return values;
  };

  const guard = async <T>(f: () => Promise<T>): Promise<T> => {
    try {
      return await f();
    } catch (e) {
      if (e instanceof BackendError) throw e;
      if (e && typeof e === "object" && "kind" in e) {
        const { kind, message } = e as { kind: unknown; message?: unknown };
        throw new BackendError(String(kind), String(message ?? kind));
      }
      throw new BackendError("auth", e instanceof Error ? e.message : String(e));
    }
  };

  return {
    tokens: () => guard(async () => (await tokens(host.session().id)).list()),
    fetchToken: (path: string, input: SendInput) =>
      guard(async () => {
        const s = host.session();
        const ws = s.ws as TokenWorkspace;
        const coreInput: CoreSendInput = {
          environment: input.environment,
          vars: input.vars,
          secrets: await secretValues(s.id, ws, input.environment),
        };
        const auth = await forSend(s.id);
        return ws.fetchToken(path, coreInput, null, { auth });
      }),
    clearTokens: (key) => guard(async () => (await tokens(host.session().id)).clear(key)),
    onPrompt: (handler) => {
      handlers.add(handler);
      return () => handlers.delete(handler);
    },
    cancelPrompt: async () => {
      cancelled = true;
      abort?.(new Error("cancelled"));
      emit({ kind: "done" });
    },
    forSend: (collectionId) => forSend(collectionId),
  };
}
