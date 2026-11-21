// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Browser side of the companion (`rt agent`): discovery, pairing, token storage.
// Protocol: crates/rt-core/src/transport/companion.rs; security model:
// crates/rt-cli/README.md ("Companion"). Owner: wave 3, strand G.
// The WebBackend (strand E) uses this for `Backend.transport` and passes `url` and `token`
// to rt-wasm, which sends through `CompanionTransport`.
//
// Pairing codes reach the client through the URL fragment `#rt-pair=<code>`: the pairing
// link `rt agent` prints, or the same fragment set by the UI when the user types the code.
// The client pairs as soon as it sees one (at startup and on `hashchange`) and removes it
// from the address bar. Pairing only stores a token; sending through the companion still
// needs the user's click (`TransportApi.useCompanion`).
//
// The token is kept in `localStorage`, one entry per companion URL (and, as all storage,
// per page origin). It is a capability for a loopback service that only accepts it from
// this origin and forgets it when `rt agent` restarts. IndexedDB would not protect it
// better: script running in this origin can read either. `localStorage` is synchronous
// and shared by the origin's tabs, so pairing in one tab (the pairing link opens a new
// one) pairs them all. If storage is unavailable, the token lives in memory.

import type { CompanionStatus } from "@requesttailor/ui";

export const DEFAULT_COMPANION_URL = "http://127.0.0.1:47812";

/** Protocol version this client speaks (`PROTOCOL_VERSION` in rt-core). */
const PROTOCOL = 1;
/** `GET /v1/status` gives up after this long: a running companion answers in milliseconds. */
const STATUS_TIMEOUT_MS = 1500;
const PAIR_TIMEOUT_MS = 5000;
/** Origins `rt agent` allows without `--allow-origin`. */
const AGENT_DEFAULT_ORIGINS = ["http://127.0.0.1:5173", "http://localhost:5173"];
const DEFAULT_PORT = "47812";
const PAIR_PARAM = "rt-pair";

export interface CompanionClient {
  status(): Promise<CompanionStatus>;
  /** Pairs this origin with the running companion; resolves when a token is stored.
   *  Uses `code`, or else a code from the URL (`#rt-pair=`); rejects if there is none. */
  pair(code?: string): Promise<void>;
  /** Base URL and token for rt-wasm, or `null` when not paired. */
  credentials(): Promise<{ url: string; token: string } | null>;
  forget(): Promise<void>;
}

/** The command the user should run, for `CompanionStatus.howTo`. */
export function agentCommand(url: string, origin: string | null): string {
  const parts = ["rt agent"];
  const port = new URL(url).port || "80";
  if (port !== DEFAULT_PORT) parts.push(`--port ${port}`);
  if (origin && !AGENT_DEFAULT_ORIGINS.includes(origin)) parts.push(`--allow-origin ${origin}`);
  return parts.join(" ");
}

/** Takes a pairing code out of the URL fragment and removes it from the address bar. */
export function takePairingCode(): string | null {
  if (typeof location === "undefined") return null;
  const params = location.hash.replace(/^#/, "").split("&");
  const entry = params.find((p) => p.startsWith(`${PAIR_PARAM}=`));
  if (entry === undefined) return null;
  const rest = params.filter((p) => p !== entry && p !== "").join("&");
  // `replaceState` does not fire `hashchange`, and keeps the code out of the history.
  history.replaceState(history.state, "", `${location.pathname}${location.search}${rest ? `#${rest}` : ""}`);
  let code = entry.slice(PAIR_PARAM.length + 1);
  try {
    code = decodeURIComponent(code);
  } catch {
    // Keep it as typed; the companion will reject it.
  }
  return code.trim() || null;
}

/** Token storage, one entry per companion URL; memory if `localStorage` is unavailable. */
function tokenStore(url: string) {
  const key = `requesttailor.companion.token:${url}`;
  let memory: string | null = null;
  return {
    get(): string | null {
      try {
        return localStorage.getItem(key);
      } catch {
        return memory;
      }
    },
    set(token: string): void {
      memory = token;
      try {
        localStorage.setItem(key, token);
      } catch {
        // Memory only.
      }
    },
    clear(): void {
      memory = null;
      try {
        localStorage.removeItem(key);
      } catch {
        // Memory only.
      }
    },
  };
}

export function createCompanionClient(url: string = DEFAULT_COMPANION_URL): CompanionClient {
  const base = url.replace(/\/+$/, "");
  const origin = typeof location === "undefined" ? null : location.origin;
  const howTo = `Run \`${agentCommand(base, origin)}\` in a terminal, then try again.`;
  const tokens = tokenStore(base);
  /** Pairing started from a code in the URL, if any. */
  let pending: Promise<void> | null = null;

  async function pairWith(code: string): Promise<void> {
    let res: Response;
    try {
      res = await fetch(`${base}/v1/pair`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ code }),
        cache: "no-store",
        signal: AbortSignal.timeout(PAIR_TIMEOUT_MS),
      });
    } catch {
      throw new Error(`The companion is not reachable. ${howTo}`);
    }
    const body: unknown = await res.json().catch(() => null);
    if (!res.ok) {
      const reason = (body as { error?: unknown } | null)?.error;
      throw new Error(
        `Pairing failed${typeof reason === "string" ? `: ${reason}` : ` (HTTP ${res.status})`}. ` +
          "Use the newest code `rt agent` printed.",
      );
    }
    const token = (body as { token?: unknown } | null)?.token;
    if (typeof token !== "string" || token === "") throw new Error("The companion sent no token.");
    tokens.set(token);
  }

  /** Why the last pairing from the URL failed, for the next `pair()`. */
  let failure: unknown = null;

  function pairFromUrl(): void {
    const code = takePairingCode();
    if (!code) return;
    const attempt: Promise<void> = pairWith(code).then(
      () => {
        failure = null;
      },
      (e: unknown) => {
        // Surfaces through `status()` (still not paired) and the next `pair()`.
        failure = e;
        console.warn("RequestTailor companion:", e);
      },
    );
    pending = attempt;
    void attempt.finally(() => {
      if (pending === attempt) pending = null;
    });
  }

  // The code is single use and expires: pair as soon as the page sees it.
  pairFromUrl();
  if (typeof window !== "undefined") window.addEventListener("hashchange", pairFromUrl);

  return {
    async status(): Promise<CompanionStatus> {
      await pending;
      const token = tokens.get();
      const unavailable: CompanionStatus = { available: false, paired: false, url: null, howTo };
      let res: Response;
      try {
        res = await fetch(`${base}/v1/status`, {
          headers: token ? { Authorization: `Bearer ${token}` } : {},
          cache: "no-store",
          signal: AbortSignal.timeout(STATUS_TIMEOUT_MS),
        });
      } catch {
        // Not running, or it refused this origin (then the browser hides the answer).
        return unavailable;
      }
      if (!res.ok) return unavailable;
      const status = (await res.json().catch(() => null)) as { protocol?: unknown; paired?: unknown } | null;
      if (status?.protocol !== PROTOCOL) {
        return { ...unavailable, howTo: `Update RequestTailor: this companion speaks another protocol. ${howTo}` };
      }
      const paired = token !== null && status.paired === true;
      // The companion forgot the token (it was restarted): drop it.
      if (token !== null && !paired) tokens.clear();
      return { available: true, paired, url: base, howTo };
    },

    async pair(code?: string): Promise<void> {
      if (code !== undefined) {
        await pairWith(code.trim());
        return;
      }
      await pending;
      if (failure !== null) {
        const e = failure;
        failure = null;
        throw e;
      }
      const fromUrl = takePairingCode();
      if (!fromUrl) {
        if (tokens.get()) return;
        throw new Error("Open the pairing link that `rt agent` printed, or enter its pairing code.");
      }
      await pairWith(fromUrl);
    },

    async credentials() {
      const token = tokens.get();
      return token ? { url: base, token } : null;
    },

    async forget(): Promise<void> {
      tokens.clear();
    },
  };
}
