// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// isomorphic-git's `http` client for the web app: every Git request goes through the
// companion's `POST /v1/send` (protocol: crates/rt-core/src/transport/companion.rs), since
// browsers cannot talk to Git hosts directly (CORS). The request body is collected and sent
// base64-encoded in a `WireRequest`; the upstream response comes back raw, its status and
// headers in the `x-rt-head` header.
//
// Errors: companion unreachable → `BackendError("unsupported")` with how to start it; token
// refused (401) → `BackendError("companion")`; the companion's transport errors (502) →
// `BackendError("git")` with its message. Git credentials only travel in the `Authorization`
// header isomorphic-git sets; nothing here logs headers or bodies.

import { BackendError } from "@requesttailor/ui";

export const NO_COMPANION_MESSAGE = "Start `rt agent` to push and pull from the web app.";

/** isomorphic-git's `GitHttpRequest`. */
export interface GitHttpRequest {
  url: string;
  method?: string;
  headers?: Record<string, string>;
  body?: AsyncIterable<Uint8Array> | Iterable<Uint8Array> | Uint8Array;
  onProgress?: unknown;
  signal?: AbortSignal;
}

/** isomorphic-git's `GitHttpResponse`. */
export interface GitHttpResponse {
  url: string;
  method?: string;
  headers: Record<string, string>;
  body: AsyncIterableIterator<Uint8Array>;
  statusCode: number;
  statusMessage: string;
}

export interface HttpClient {
  request(request: GitHttpRequest): Promise<GitHttpResponse>;
}

export interface Companion {
  url: string;
  token: string;
}

type Fetch = typeof fetch;

async function collect(body: GitHttpRequest["body"]): Promise<Uint8Array | null> {
  if (body == null) return null;
  if (body instanceof Uint8Array) return body;
  const chunks: Uint8Array[] = [];
  for await (const chunk of body as AsyncIterable<Uint8Array>) chunks.push(chunk);
  const out = new Uint8Array(chunks.reduce((n, c) => n + c.length, 0));
  let at = 0;
  for (const c of chunks) {
    out.set(c, at);
    at += c.length;
  }
  return out;
}

export function toBase64(bytes: Uint8Array): string {
  let out = "";
  const step = 0x8000;
  for (let i = 0; i < bytes.length; i += step) {
    out += String.fromCharCode(...bytes.subarray(i, i + step));
  }
  return btoa(out);
}

async function* chunks(body: ReadableStream<Uint8Array> | null): AsyncIterableIterator<Uint8Array> {
  if (!body) return;
  const reader = body.getReader();
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) return;
      if (value && value.length > 0) yield value;
    }
  } finally {
    reader.releaseLock();
  }
}

interface WireHead {
  status: number;
  headers: [string, string][];
  url: string;
}

const STATUS_TEXT: Record<number, string> = {
  200: "OK",
  401: "Unauthorized",
  403: "Forbidden",
  404: "Not Found",
  500: "Internal Server Error",
};

/** An `http` client for isomorphic-git that sends through `companion`. */
export function companionHttp(companion: Companion, fetchImpl: Fetch = (...a) => fetch(...a)): HttpClient {
  const endpoint = `${companion.url.replace(/\/+$/, "")}/v1/send`;
  return {
    async request(req) {
      const body = await collect(req.body);
      const headers = Object.entries(req.headers ?? {}).map(([k, v]) => [k, String(v)]);
      const wire = {
        method: req.method ?? "GET",
        url: req.url,
        headers,
        body: body ? toBase64(body) : null,
        content_type: null,
        timeout_ms: null,
        follow_redirects: true,
        max_redirects: 10,
      };
      let res: Response;
      try {
        res = await fetchImpl(endpoint, {
          method: "POST",
          headers: { Authorization: `Bearer ${companion.token}`, "Content-Type": "application/json" },
          body: JSON.stringify(wire),
          cache: "no-store",
          signal: req.signal,
        });
      } catch (e) {
        if (req.signal?.aborted) throw e;
        throw new BackendError("unsupported", `The companion is not reachable. ${NO_COMPANION_MESSAGE}`);
      }
      if (res.status === 401) {
        throw new BackendError(
          "companion",
          "The companion no longer accepts this browser's pairing. Pair again (Send through the companion), then retry.",
        );
      }
      if (res.status === 502) {
        const error = (await res.json().catch(() => null)) as { kind?: string; message?: unknown } | null;
        const detail = typeof error?.message === "string" ? error.message : (error?.kind ?? "unknown error");
        throw new BackendError("git", `Could not reach ${hostOf(req.url)}: ${detail}`);
      }
      if (res.status !== 200) {
        const text = await res.text().catch(() => "");
        throw new BackendError("git", `The companion answered HTTP ${res.status}${text ? `: ${text}` : ""}`);
      }
      const raw = res.headers.get("x-rt-head");
      if (!raw) throw new BackendError("git", "The companion sent no x-rt-head header.");
      const head = JSON.parse(raw) as WireHead;
      const out: Record<string, string> = {};
      for (const [k, v] of head.headers) {
        const key = k.toLowerCase();
        out[key] = key in out ? `${out[key]}, ${v}` : v;
      }
      return {
        url: head.url,
        method: req.method,
        headers: out,
        body: chunks(res.body),
        statusCode: head.status,
        statusMessage: STATUS_TEXT[head.status] ?? "",
      };
    },
  };
}

/** `host[:port]` of a URL, the key for stored Git credentials. */
export function hostOf(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}
