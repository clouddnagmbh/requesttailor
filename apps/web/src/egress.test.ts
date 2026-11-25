// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand B: the web shell around rt-wasm's egress functions. The core is replaced by a
// small fake with the same contract (the real one runs in `e2e/egress/`): the list of allowed
// hosts lives in browser storage, a call the browser refuses (CORS) is repeated over a paired
// companion, the distribution's guard is passed with every call, and secret values only come
// for the names the headers refer to.

import type { EgressHost, EgressRequest, EgressResponse } from "@requesttailor/ui";
import { BackendError } from "@requesttailor/ui";
import { describe, expect, it } from "vitest";

import { HOSTS_KEY, webEgress, type CoreEgress, type EgressDeps } from "./egress";
import type { CompanionCredentials } from "./wasm";

const CANARY = "RT-CANARY-web-egress-91ac";
const CREDENTIALS: CompanionCredentials = { url: "http://127.0.0.1:48707", token: "t" };

function named(kind: string, message: string): Error {
  return Object.assign(new Error(message), { kind });
}

interface Seen {
  request: EgressRequest;
  hosts: EgressHost[];
  secrets: [string, string][];
  collection: string;
  companion: CompanionCredentials | null;
  guard: boolean;
}

function fakeCore(answer: (seen: Seen) => EgressResponse | Error): { core: CoreEgress; seen: Seen[] } {
  const seen: Seen[] = [];
  const core: CoreEgress = {
    egressHostsParse: (text) => {
      const parsed: unknown = JSON.parse(text);
      if (!Array.isArray(parsed)) throw named("shell", "the list of allowed hosts cannot be read");
      return parsed as EgressHost[];
    },
    egressHostsAllow: (hosts, host, port, extension, purpose, nowMs) => {
      const name = host.toLowerCase();
      const entry: EgressHost = { host: name, ...(port === null ? {} : { port }), extension, purpose, allowed_at: nowMs };
      const have = hosts.find((h) => h.host === name && (h.port ?? null) === port);
      return { hosts: have ? hosts : [...hosts, entry], entry: have ?? entry };
    },
    egressHostsRemove: (hosts, host, port) => hosts.filter((h) => !(h.host === host && (h.port ?? null) === port)),
    egressSecretNames: (request) =>
      (request.headers ?? []).flatMap(([, v]) => [...v.matchAll(/\{\{secret:([^}]+)\}\}/g)].map((m) => m[1])),
    egressRequest: async (request, hosts, secrets, collection, companion, services) => {
      const one: Seen = { request, hosts, secrets, collection, companion, guard: services !== null };
      seen.push(one);
      const out = answer(one);
      if (out instanceof Error) throw out;
      return out;
    },
  };
  return { core, seen };
}

const OK: EgressResponse = { status: 200, headers: [], body: "{}", took_ms: 1 };

function memoryStorage() {
  const map = new Map<string, string>();
  return { map, getItem: (k: string) => map.get(k) ?? null, setItem: (k: string, v: string) => void map.set(k, v) };
}

function setup(
  answer: (seen: Seen) => EgressResponse | Error,
  opts: { on?: boolean; paired?: CompanionCredentials | null; guard?: boolean; storage?: ReturnType<typeof memoryStorage> | null } = {},
) {
  const { core, seen } = fakeCore(answer);
  const storage = opts.storage === undefined ? memoryStorage() : opts.storage;
  const asked: string[][] = [];
  const deps: EgressDeps = {
    core: async () => core,
    secrets: async (_request, names) => {
      asked.push(names);
      return { collection: "Orders", values: names.map((n): [string, string] => [n, CANARY]) };
    },
    companion: {
      on: () => opts.on ?? false,
      active: async () => (opts.on ? CREDENTIALS : null),
      paired: async () => (opts.paired === undefined ? null : opts.paired),
    },
    sendGuard: () => (opts.guard ? () => ({ decision: "allow" }) : undefined),
    storage,
    now: () => 1_700_000_000_000,
  };
  return { api: webEgress(deps), seen, storage, asked };
}

const GET = (url: string, headers: [string, string][] = []): EgressRequest => ({
  method: "GET",
  url,
  headers,
  extension: "traces",
  purpose: "Server traces",
});

describe("the list of allowed hosts", () => {
  it("is kept in browser storage and read back", async () => {
    const { api, storage } = setup(() => OK);
    expect(await api.hosts()).toEqual([]);
    const entry = await api.allow("Traces.Example", null, "traces", "Server traces");
    expect(entry).toMatchObject({ host: "traces.example", extension: "traces", allowed_at: 1_700_000_000_000 });
    expect(JSON.parse(storage!.map.get(HOSTS_KEY)!)).toHaveLength(1);
    await api.allow("traces.example", null, "other", "Other");
    expect(await api.hosts()).toHaveLength(1);
    await api.remove("traces.example", null);
    expect(await api.hosts()).toEqual([]);
  });

  it("falls back to memory when storage refuses, and a broken value allows nothing", async () => {
    const refusing = {
      getItem: () => {
        throw new Error("blocked");
      },
      setItem: () => {
        throw new Error("blocked");
      },
    };
    const { api } = setup(() => OK, { storage: refusing as unknown as ReturnType<typeof memoryStorage> });
    await api.allow("a.example", null, "x", "y");
    expect((await api.hosts()).map((h) => h.host)).toEqual(["a.example"]);
    const broken = memoryStorage();
    broken.map.set(HOSTS_KEY, "{ not a list");
    const second = setup(() => OK, { storage: broken });
    await expect(second.api.hosts()).rejects.toBeInstanceOf(BackendError);
  });
});

describe("a call", () => {
  it("hands the list, the guard and only the secret names' values to the core", async () => {
    const { api, seen, asked } = setup(() => OK, { guard: true });
    await api.allow("traces.example", null, "traces", "Server traces");
    await api.request(GET("https://traces.example/a"));
    expect(asked).toEqual([]);
    expect(seen[0].secrets).toEqual([]);
    expect(seen[0].guard).toBe(true);
    expect(seen[0].hosts.map((h) => h.host)).toEqual(["traces.example"]);
    await api.request(GET("https://traces.example/b", [["Authorization", "Bearer {{secret:token}}"]]));
    expect(asked).toEqual([["token"]]);
    expect(seen[1].secrets).toEqual([["token", CANARY]]);
    expect(seen[1].collection).toBe("Orders");
    // The request the core got still holds the reference, never the value.
    expect(JSON.stringify(seen[1].request)).not.toContain(CANARY);
  });

  it("passes the core's kinds on (not-allowed, blocked, limit)", async () => {
    for (const kind of ["not-allowed", "blocked", "limit"]) {
      const { api } = setup(() => named(kind, `${kind} message`));
      const error = await api.request(GET("https://x.example/")).catch((e: unknown) => e);
      expect(error).toBeInstanceOf(BackendError);
      expect((error as BackendError).kind).toBe(kind);
    }
  });

  it("goes over the companion while it is switched on", async () => {
    const { api, seen } = setup(() => OK, { on: true });
    await api.request(GET("https://traces.example/"));
    expect(seen).toHaveLength(1);
    expect(seen[0].companion).toEqual(CREDENTIALS);
  });

  it("repeats a call the browser refused over a paired companion", async () => {
    const { api, seen } = setup((s) => (s.companion ? OK : named("transport", "blocked by the browser: Failed to fetch")), { paired: CREDENTIALS });
    const answer = await api.request(GET("https://traces.example/"));
    expect(answer.status).toBe(200);
    expect(seen.map((s) => s.companion)).toEqual([null, CREDENTIALS]);
  });

  it("says what to do when the browser refuses and no companion is paired", async () => {
    const { api, seen } = setup(() => named("transport", "blocked by the browser: Failed to fetch"));
    const error = (await api.request(GET("https://traces.example/")).catch((e: unknown) => e)) as BackendError;
    expect(error.kind).toBe("transport");
    expect(error.message).toContain("rt agent");
    expect(seen).toHaveLength(1);
  });

  it("does not loop when the companion cannot reach it either", async () => {
    const { api, seen } = setup(() => named("transport", "blocked by the browser: nope"), { on: true });
    const error = (await api.request(GET("https://traces.example/")).catch((e: unknown) => e)) as BackendError;
    expect(error.kind).toBe("transport");
    expect(seen).toHaveLength(1);
  });

  it("leaves other transport errors alone (no second call)", async () => {
    const { api, seen } = setup(() => named("transport", "request timed out"), { paired: CREDENTIALS });
    await expect(api.request(GET("https://traces.example/"))).rejects.toMatchObject({ kind: "transport" });
    expect(seen).toHaveLength(1);
  });

  it("sends a writing call over a paired companion at once, never twice", async () => {
    const { api, seen } = setup(() => OK, { paired: CREDENTIALS });
    await api.request({ ...GET("https://traces.example/"), method: "POST", body: "{}" });
    expect(seen.map((s) => s.companion)).toEqual([CREDENTIALS]);
  });

  it("does not repeat a refused writing call without a companion", async () => {
    const { api, seen } = setup(() => named("transport", "blocked by the browser: Failed to fetch"));
    const error = (await api.request({ ...GET("https://traces.example/"), method: "POST", body: "{}" }).catch((e: unknown) => e)) as BackendError;
    expect(error.kind).toBe("transport");
    expect(error.message).toContain("not repeated");
    expect(seen).toHaveLength(1);
  });
});
