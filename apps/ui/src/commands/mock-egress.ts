// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `EgressApi` of the mock backend (wave 19, strand B): the rules of `rt_app::egress` on an
// in-memory list of hosts, for UI tests. A host that is not allowed answers `not-allowed`; the
// host `blocked.example` plays a guard that refuses (`blocked`); a `{{secret:<name>}}` header is
// "resolved" to a value that is masked in the answer; `GET`/`POST` only, no redirects.

import type { EgressApi } from "./backend";
import { BackendError } from "./backend";
import type { EgressHost, EgressResponse } from "./types";

const MASK = "••••";

export function mockEgress(): EgressApi {
  let hosts: EgressHost[] = [];
  const same = (h: EgressHost, host: string, port: number | null) => h.host === host && (h.port ?? null) === port;
  return {
    hosts: async () => hosts.map((h) => ({ ...h })),
    allow: async (host, port, extension, purpose) => {
      const name = host.trim().replace(/\.$/, "").toLowerCase();
      if (!name || /[\s/\\?#@]/.test(name)) throw new BackendError("shell", `${host} is not a host name (no scheme, path or spaces)`);
      const existing = hosts.find((h) => same(h, name, port));
      if (existing) return { ...existing };
      const entry: EgressHost = { host: name, ...(port === null ? {} : { port }), extension, purpose, allowed_at: Date.now() };
      hosts = [...hosts, entry];
      return { ...entry };
    },
    remove: async (host, port) => {
      hosts = hosts.filter((h) => !same(h, host.toLowerCase(), port));
    },
    request: async (request): Promise<EgressResponse> => {
      if (request.method !== "GET" && request.method !== "POST") throw new BackendError("limit", "only GET and POST are allowed");
      let url: URL;
      try {
        url = new URL(request.url);
      } catch {
        throw new BackendError("limit", "the URL is not valid");
      }
      const port = url.port ? Number(url.port) : url.protocol === "http:" ? 80 : null;
      const effective = port ?? 443;
      if (!hosts.some((h) => h.host === url.hostname && (h.port ?? 443) === effective)) {
        throw new BackendError("not-allowed", `${url.hostname} is not an allowed host`);
      }
      if (url.hostname === "blocked.example") throw new BackendError("blocked", "blocked: no outbound calls in this mode");
      const names = (request.headers ?? []).map(([n]) => n);
      const used = (request.headers ?? []).flatMap(([, v]) => [...v.matchAll(/\{\{\s*secret:([^}]*?)\s*\}\}/g)].map((m) => m[1]));
      return {
        status: 200,
        headers: [["content-type", "application/json"], ...used.map((n): [string, string] => ["x-echo-" + n, MASK])],
        body: JSON.stringify({ method: request.method, url: request.url, headers: names, secrets: used.map((n) => `${n}=${MASK}`) }),
        truncated: false,
        took_ms: 1,
      };
    },
  };
}
