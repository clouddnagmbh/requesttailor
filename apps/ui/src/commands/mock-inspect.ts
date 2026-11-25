// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `InspectApi` of the mock backend (wave 18, strand B): a small mirror of `rt_app::inspect` for UI
// tests (`?backend=mock`). Tokens are read from the request as the mock sees it (headers, bearer
// auth, query, with `{{name}}` replaced from the runtime and environment variables) and from the
// kept response; the checks are the nine rules of the core in a simpler form (no stack-trace
// scan beyond a few markers, no 1 MiB cut). Rules switched off live in memory. The core is the
// reference; this only has to behave the same way for the screens.

import { BackendError, type InspectApi } from "./backend";
import type {
  CheckScope,
  ClaimKind,
  Environment,
  FoundToken,
  JwtClaim,
  JwtInfo,
  JwtWarning,
  Observation,
  RequestSpec,
  TokenLocation,
} from "./types";

/** What the mock inspection uses of the mock backend. */
export interface MockInspectHost {
  getRequest(path: string): Promise<RequestSpec>;
  environments(): Promise<Environment[]>;
  /** A kept response: status, headers, final URL, body text (`null` for none or binary). */
  response(id: number): { status: number; headers: [string, string][]; url: string; text: string | null } | null;
}

const WELL_KNOWN: [string, string][] = [
  ["iss", "Issuer: who created the token"],
  ["sub", "Subject: who the token is about"],
  ["aud", "Audience: who the token is meant for"],
  ["iat", "Issued at"],
  ["nbf", "Not before: invalid until this time"],
  ["exp", "Expires: invalid from this time"],
  ["scope", "Scopes the token grants"],
  ["scp", "Scopes the token grants"],
  ["azp", "Authorized party: the client the token was issued to"],
];
const TIME_CLAIMS = ["iat", "nbf", "exp", "auth_time", "updated_at"];

function decodePart(part: string): unknown {
  const base = part.replace(/-/g, "+").replace(/_/g, "/");
  const padded = base + "=".repeat((4 - (base.length % 4)) % 4);
  const text = decodeURIComponent(
    atob(padded)
      .split("")
      .map((c) => "%" + c.charCodeAt(0).toString(16).padStart(2, "0"))
      .join(""),
  );
  return JSON.parse(text);
}

function span(seconds: number): string {
  const s = Math.floor(Math.abs(seconds));
  if (s < 60) return `${s} s`;
  if (s < 3600) return `${Math.floor(s / 60)} min`;
  if (s < 86_400) return `${Math.floor(s / 3600)} h`;
  return `${Math.floor(s / 86_400)} d`;
}

const now = () => Math.floor(Date.now() / 1000);

/** Reads a token as `rt_core::jwt::parse` does; `null` if it is not one. */
export function mockParseJwt(raw: string): JwtInfo | null {
  let token = raw.trim();
  if (/^bearer\s/i.test(token)) token = token.slice(7).trim();
  if (token.length > 64 * 1024) return null;
  const parts = token.split(".");
  if (parts.length !== 3 || !parts[0] || !parts[1]) return null;
  let header: unknown;
  let claims: unknown;
  try {
    header = decodePart(parts[0]);
    claims = decodePart(parts[1]);
  } catch {
    return null;
  }
  if (typeof header !== "object" || header === null || Array.isArray(header)) return null;
  if (typeof claims !== "object" || claims === null || Array.isArray(claims)) return null;
  const h = header as Record<string, unknown>;
  const c = claims as Record<string, unknown>;
  const kind = (name: string, v: unknown): ClaimKind =>
    TIME_CLAIMS.includes(name) && typeof v === "number" ? "time" : Array.isArray(v) || (name === "scope" && typeof v === "string") ? "list" : typeof v === "string" ? "text" : "other";
  const list: JwtClaim[] = [];
  for (const [name, meaning] of WELL_KNOWN) if (name in c) list.push({ name, value: c[name], kind: kind(name, c[name]), meaning });
  for (const name of Object.keys(c).sort()) if (!WELL_KNOWN.some(([n]) => n === name)) list.push({ name, value: c[name], kind: kind(name, c[name]), meaning: null });
  const t = now();
  const exp = typeof c.exp === "number" ? Math.floor(c.exp) : null;
  const nbf = typeof c.nbf === "number" ? Math.floor(c.nbf) : null;
  const warnings: JwtWarning[] = [];
  if (exp !== null) {
    if (exp <= t) warnings.push({ kind: "expired", text: `token expired ${span(t - exp)} ago (exp)` });
    else if (exp - t < 30) warnings.push({ kind: "expires-soon", text: `token expires in ${span(exp - t)} (exp)` });
  }
  if (nbf !== null && nbf > t) warnings.push({ kind: "not-yet-valid", text: `token is not valid yet, nbf is in ${span(nbf - t)}` });
  const alg = typeof h.alg === "string" ? h.alg : null;
  if (alg?.toLowerCase() === "none") warnings.push({ kind: "algorithm-none", text: "token header says alg none: it is not signed" });
  return { header, algorithm: alg, claims: list, warnings, expires_at: exp, signature_verified: false };
}

function substitute(text: string, vars: Map<string, string>): string {
  return text.replace(/\{\{\s*([^}\s]+)\s*\}\}/g, (m, name: string) => vars.get(name) ?? m);
}

const HEADER_KEYS = ["access_token", "id_token", "token"];

function walk(value: unknown, path: string, depth: number, out: FoundToken[]): void {
  if (depth > 6 || out.length >= 16 || typeof value !== "object" || value === null) return;
  if (Array.isArray(value)) {
    value.forEach((v, i) => walk(v, `${path}[${i}]`, depth + 1, out));
    return;
  }
  for (const [k, v] of Object.entries(value)) {
    const here = `${path}.${k}`;
    if (typeof v === "string" && HEADER_KEYS.includes(k)) {
      const token = mockParseJwt(v);
      if (token) out.push({ location: { kind: "body", path: here }, token });
    } else walk(v, here, depth + 1, out);
  }
}

// ---- the checks ----

const obs = (id: string, severity: "info" | "warn", location: string, why: string, fix: string): Observation => ({ id, severity, location, why, fix });

export const MOCK_RULES = [
  "chk/hsts-missing",
  "chk/nosniff-missing",
  "chk/csp-missing",
  "chk/frame-protection-missing",
  "chk/cookie-flags",
  "chk/cors-credentials",
  "chk/version-disclosure",
  "chk/stack-trace",
  "chk/jwt-alg-none",
];

function checkResponse(r: { status: number; headers: [string, string][]; url: string; text: string | null }, ignore: Set<string>): Observation[] {
  const header = (n: string) => r.headers.find(([k]) => k.toLowerCase() === n)?.[1];
  const all = (n: string) => r.headers.filter(([k]) => k.toLowerCase() === n).map(([, v]) => v);
  const https = /^https:\/\//i.test(r.url);
  const html = /text\/html|application\/xhtml/i.test(header("content-type") ?? "");
  const representation = ![204, 304].includes(r.status) && r.status >= 200;
  const out: Observation[] = [];
  const on = (id: string) => !ignore.has(id);
  if (on("chk/hsts-missing") && https && representation && header("strict-transport-security") === undefined)
    out.push(obs("chk/hsts-missing", "warn", "response headers", "Without Strict-Transport-Security a browser may still try plain http for this host.", "Send Strict-Transport-Security: max-age=31536000; includeSubDomains over https."));
  if (on("chk/nosniff-missing") && representation && !all("x-content-type-options").some((v) => v.trim().toLowerCase() === "nosniff"))
    out.push(obs("chk/nosniff-missing", "info", "response headers", "A browser may guess a content type other than the declared one and run the body as script.", "Send X-Content-Type-Options: nosniff."));
  if (on("chk/csp-missing") && html && representation && header("content-security-policy") === undefined)
    out.push(obs("chk/csp-missing", "info", "response headers", "An HTML page without a Content-Security-Policy gives injected script free rein.", "Send a Content-Security-Policy that limits script sources, starting with default-src 'self'."));
  if (on("chk/frame-protection-missing") && html && representation && header("x-frame-options") === undefined && !all("content-security-policy").some((v) => /frame-ancestors/i.test(v)))
    out.push(obs("chk/frame-protection-missing", "info", "response headers", "Another site can show this page in a frame and trick people into clicking it.", "Send X-Frame-Options: DENY or a Content-Security-Policy with frame-ancestors 'none'."));
  if (on("chk/cookie-flags"))
    for (const cookie of all("set-cookie")) {
      const [first = "", ...attrs] = cookie.split(";");
      const keys = attrs.map((a) => a.split("=")[0]!.trim().toLowerCase());
      const missing = [https && !keys.includes("secure") ? "Secure" : "", !keys.includes("httponly") ? "HttpOnly" : "", !keys.includes("samesite") ? "SameSite" : ""].filter(Boolean);
      if (missing.length > 0)
        out.push(obs("chk/cookie-flags", "warn", `header Set-Cookie (${first.split("=")[0]!.trim()})`, `The cookie lacks ${missing.join(", ")}, so it may leak over http, be read by script or travel with cross-site requests.`, "Add the missing attributes, e.g. Secure; HttpOnly; SameSite=Lax."));
    }
  if (on("chk/cors-credentials") && header("access-control-allow-origin")?.trim() === "*" && header("access-control-allow-credentials")?.trim().toLowerCase() === "true")
    out.push(obs("chk/cors-credentials", "warn", "header Access-Control-Allow-Origin", "Access-Control-Allow-Origin * together with Allow-Credentials true: any site may be allowed to read answers that depend on the user's cookies.", "Answer with an explicit list of trusted origins, or drop Access-Control-Allow-Credentials."));
  if (on("chk/version-disclosure"))
    for (const [name, label] of [["server", "Server"], ["x-powered-by", "X-Powered-By"]] as const)
      for (const v of all(name))
        if (/\d\.\d/.test(v)) out.push(obs("chk/version-disclosure", "info", `header ${label}`, "The header names the software and its version, which tells an attacker which known flaws to try.", "Remove the version, or the header, in the server or proxy configuration."));
  if (on("chk/stack-trace") && r.status >= 400 && r.text !== null) {
    const lines = r.text.split("\n");
    const at = lines.findIndex((l) => /^\s*at .*(\.java:|\.cs:line |\.js:|node:internal)/.test(l) || /^\s*Traceback \(most recent call last\)/.test(l) || /^\s*Stack trace:$/.test(l));
    if (at >= 0)
      out.push(obs("chk/stack-trace", "warn", `body, line ${at + 1}`, "The error answer shows internal file names and call frames, which helps an attacker map the application.", "Return a short error with an id and keep the trace in the server log."));
  }
  if (on("chk/jwt-alg-none")) {
    const places: [string, string][] = [...r.headers.map(([n, v]): [string, string] => [`header ${n}`, v]), ["body", r.text ?? ""]];
    for (const [place, text] of places) {
      const hit = [...text.matchAll(/(?<![A-Za-z0-9_.-])eyJ[A-Za-z0-9_-]*\.[A-Za-z0-9_-]*\.[A-Za-z0-9_-]*/g)].some((m) => mockParseJwt(m[0])?.warnings.some((w) => w.kind === "algorithm-none"));
      if (hit) out.push(obs("chk/jwt-alg-none", "warn", place, "A token whose header says alg none carries no signature, so anyone can forge its claims.", "Issue signed tokens and make the verifier reject alg none."));
    }
  }
  return out;
}

export function mockInspect(host?: MockInspectHost): InspectApi {
  const device = new Set<string>();
  const collection = new Set<string>();
  const no = (what: string) => Promise.reject(new BackendError("unsupported", `${what} needs a mock backend host.`));
  return {
    jwt: async (token) => {
      const info = mockParseJwt(token);
      if (!info) throw new BackendError("shell", "not a JSON Web Token");
      return info;
    },
    tokens: async (target) => {
      if (!host) return no("Tokens");
      const out: FoundToken[] = [];
      let spec: RequestSpec;
      try {
        spec = await host.getRequest(target.path);
      } catch {
        return out;
      }
      const envs = await host.environments();
      const vars = new Map<string, string>();
      const env = envs.find((e) => e.name === target.environment);
      for (const v of env?.variables ?? []) if (v.enabled && !v.secret) vars.set(v.name, v.value ?? "");
      const add = (location: TokenLocation, raw: string) => {
        const token = mockParseJwt(substitute(raw, vars));
        if (token) out.push({ location, token });
      };
      if (spec.auth.type === "bearer") add({ kind: "auth" }, spec.auth.token);
      for (const h of spec.headers) if (h.enabled && h.name.toLowerCase() === "authorization" && /^bearer\s/i.test(h.value.trim())) add({ kind: "authorization" }, h.value.trim().slice(7));
      for (const m of substitute(spec.url, vars).matchAll(/[?&]access_token=([^&#]*)/g)) add({ kind: "query" }, decodeURIComponent(m[1]!));
      for (const p of spec.params) if (p.enabled && p.name === "access_token") add({ kind: "query" }, p.value ?? "");
      if (target.response_id != null) {
        const r = host.response(target.response_id);
        if (r) {
          for (const [n, v] of r.headers) {
            if (n.toLowerCase() !== "set-cookie") continue;
            const [pair = ""] = v.split(";");
            const eq = pair.indexOf("=");
            const token = mockParseJwt(pair.slice(eq + 1));
            if (token) out.push({ location: { kind: "cookie", name: pair.slice(0, Math.max(eq, 0)).trim() }, token });
          }
          if (r.text !== null && /^\s*[{[]/.test(r.text))
            try {
              walk(JSON.parse(r.text), "$", 0, out);
            } catch {
              // Not JSON after all.
            }
        }
      }
      return out;
    },
    checks: async (target) => {
      if (!host || target.response_id == null) return [];
      const r = host.response(target.response_id);
      return r ? checkResponse(r, new Set([...device, ...collection])) : [];
    },
    ignoreCheck: async (id: string, scope: CheckScope) => {
      if (!MOCK_RULES.includes(id)) throw new BackendError("shell", `unknown check: ${id}`);
      (scope === "device" ? device : collection).add(id);
    },
  };
}
