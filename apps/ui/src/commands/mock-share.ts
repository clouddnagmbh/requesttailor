// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ShareApi` of the mock backend (wave 17, strand C), for UI tests. It follows the rules of
// `rt_app::share` closely enough to test the app around it (what is in a link, what is
// refused, how a snippet reads); the text is `1.` + base64url(JSON) without deflate, so it
// is not the real format. Nothing is requested.

import { BackendError, type ShareApi } from "./backend";
import type { CopiedAs, CopyKind, Environment, RequestSpec, ShareLink, SharePayload, ShareVariable } from "./types";

const SENSITIVE = /key|token|secret|password|passwd|credential|signature|cookie|jwt|bearer|^(authorization|auth|sig|pwd|sid|sessionid)$/i;
const MAX_LINK = 8000;
const MAX_QR = 2000;

const isClean = (v: string) => v.replace(/\{\{[^}]*\}\}/g, "").trim().replace(/^(bearer|basic|token|digest|apikey)$/i, "") === "";
const slug = (n: string) =>
  n
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^_|_$/g, "") || "secret";
const hole = (n: string) => `{{secret:${slug(n)}}}`;

/** The request without credentials (the rules of `rt_app::share::sanitize`, shortened). */
export function sanitize(spec: RequestSpec): RequestSpec {
  const s = JSON.parse(JSON.stringify(spec)) as RequestSpec;
  s.seq = null;
  s.url = s.url
    .replace(/(:\/\/[^/:@]+:)([^@/]*)@/, (_m, a: string, p: string) => `${a}${isClean(p) ? p : hole("password")}@`)
    .replace(/([?&])([^=&#]+)=([^&#]*)/g, (m, sep: string, k: string, v: string) =>
      SENSITIVE.test(k) && !isClean(v) ? `${sep}${k}=${hole(k)}` : m,
    );
  for (const h of s.headers) if (SENSITIVE.test(h.name) && !isClean(h.value)) h.value = hole(h.name);
  for (const p of s.params) if (SENSITIVE.test(p.name) && !isClean(p.value)) p.value = hole(p.name);
  const a = s.auth;
  if (a.type === "basic" && !isClean(a.password)) a.password = hole("password");
  if (a.type === "bearer" && !isClean(a.token)) a.token = hole("token");
  if (a.type === "api-key" && !isClean(a.value)) a.value = hole(a.key || "api_key");
  if (s.body.type === "raw") {
    s.body.data = s.body.data.replace(/("([^"\\]*)"\s*:\s*)"([^"\\]*)"/g, (m, pre: string, k: string, v: string) =>
      SENSITIVE.test(k) && !isClean(v) ? `${pre}"${hole(k)}"` : m,
    );
  }
  for (const v of s.vars) {
    if (v.secret || (SENSITIVE.test(v.name) && v.value && !isClean(v.value))) {
      v.secret = true;
      v.value = null;
    }
  }
  return s;
}

function names(value: unknown, out: Set<string>): void {
  if (typeof value === "string") for (const m of value.matchAll(/\{\{\s*([^}\n]+?)\s*\}\}/g)) out.add(m[1]);
  else if (Array.isArray(value)) value.forEach((x) => names(x, out));
  else if (value && typeof value === "object") Object.values(value).forEach((x) => names(x, out));
}

const b64 = (text: string) =>
  btoa(String.fromCharCode(...new TextEncoder().encode(text)))
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
const unb64 = (text: string) =>
  new TextDecoder().decode(Uint8Array.from(atob(text.replace(/-/g, "+").replace(/_/g, "/")), (c) => c.charCodeAt(0)));

const envName = (n: string) =>
  n
    .replace(/^secret:/, "")
    .replace(/([a-z0-9])([A-Z])/g, "$1_$2")
    .replace(/[^A-Za-z0-9]+/g, "_")
    .toUpperCase();

export function mockShare(deps: {
  getRequest(path: string): Promise<RequestSpec>;
  environments(): Promise<Environment[]>;
}): ShareApi {
  const env = async (name?: string | null) => {
    if (!name) return null;
    const found = (await deps.environments()).find((e) => e.name === name);
    if (!found) throw new BackendError("environment-not-found", `environment not found: ${name}`);
    return found;
  };
  return {
    async link(path, options = {}) {
      const spec = await deps.getRequest(path);
      const e = await env(options.environment);
      const request = sanitize(spec);
      const used = new Set<string>();
      names(request, used);
      const variables: ShareVariable[] = [...used].sort().map((raw) => {
        const name = raw.replace(/^secret:/, "");
        const known = e?.variables.find((v) => v.name === name && v.enabled);
        const secret = raw.startsWith("secret:") || SENSITIVE.test(name) || !!known?.secret;
        return { name, secret, value: options.include_values && known && !secret ? (known.value ?? null) : null };
      });
      const payload: SharePayload = { version: 1, name: spec.name, request, variables };
      const preview = JSON.stringify(payload, null, 2);
      if (/ghp_[A-Za-z0-9]{20,}/.test(preview)) throw new BackendError("secrets-found", "Commit refused: 1 credential in 1 file (request.json:1).");
      const body = `1.${b64(JSON.stringify(payload))}`;
      const text = options.base ? `${options.base.split("#")[0]}#rt=${body}` : `rt:${body}`;
      if (text.length > MAX_LINK)
        throw new BackendError("format", `this request is too large for a link (${text.length} characters); save it as a file instead`);
      return { text, preview, payload, length: text.length, fits_qr: text.length <= MAX_QR } satisfies ShareLink;
    },
    async open(input) {
      const t = input.trim();
      const m = /(?:^rt:|[#&]rt=|^rt=)([0-9]+)\.([A-Za-z0-9_-]+)/.exec(t);
      if (!m) throw new BackendError("format", "this is not a RequestTailor link");
      if (m[1] !== "1") throw new BackendError("format", `this link needs a newer version of the app (format ${m[1]})`);
      try {
        return JSON.parse(unb64(m[2])) as SharePayload;
      } catch {
        throw new BackendError("format", "the link is damaged (cannot be unpacked)");
      }
    },
    async copyAs(path, kind: CopyKind, options = {}) {
      const spec = sanitize(await deps.getRequest(path));
      const e = await env(options.environment);
      const resolve = (t: string) =>
        t.replace(/\{\{\s*([^}\n]+?)\s*\}\}/g, (_m, n: string) => {
          const v = e?.variables.find((x) => x.name === n && x.enabled && !x.secret && !SENSITIVE.test(x.name));
          if (options.resolve_variables && v?.value != null) return v.value;
          return kind === "curl" ? `$${envName(n)}` : kind === "fetch" ? `\${process.env.${envName(n)}}` : `\${${envName(n)}}`;
        });
      if (spec.protocol && spec.protocol.type !== "http" && kind !== "curl") {
        throw new BackendError("unsupported", "not available for this request");
      }
      const headers = spec.headers.filter((h) => h.enabled);
      const body = spec.body.type === "raw" ? resolve(spec.body.data) : "";
      let text: string;
      if (kind === "curl") {
        const lines = [`curl${spec.method === "GET" ? "" : ` -X ${spec.method}`} "${resolve(spec.url)}"`];
        for (const h of headers) lines.push(`-H "${h.name}: ${resolve(h.value)}"`);
        if (body) lines.push(`--data-raw '${body.replace(/'/g, "'\\''")}'`);
        text = lines.join(" \\\n  ");
      } else if (kind === "fetch") {
        const hs = headers.map((h) => `"${h.name}": \`${resolve(h.value)}\``).join(", ");
        text = `const response = await fetch(\`${resolve(spec.url)}\`, {\n  method: "${spec.method}",\n  headers: { ${hs} },${body ? `\n  body: \`${body}\`,` : ""}\n});\nconsole.log(await response.text());\n`;
      } else {
        const hs = headers.map((h) => `"${h.name}": f"${resolve(h.value)}"`).join(", ");
        text = `import os\nimport requests\n\nresponse = requests.request("${spec.method}", f"${resolve(spec.url)}", headers={${hs}}${body ? `, data=f'${body}'` : ""})\nprint(response.text)\n`;
      }
      return { kind, text, notes: [] } satisfies CopiedAs;
    },
    async qr(link) {
      if (link.length > MAX_QR) throw new BackendError("unsupported", "too long for a QR code, copy the link or save a file");
      return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8" data-testid="qr-svg"><rect width="8" height="8" fill="#fff"/><rect x="1" y="1" width="2" height="2"/><rect x="5" y="1" width="2" height="2"/><rect x="1" y="5" width="2" height="2"/></svg>`;
    },
  };
}
