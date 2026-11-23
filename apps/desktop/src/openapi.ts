// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `OpenApiApi` of the desktop app (src-tauri/src/openapi.rs): one Tauri command per method.
// The spec's files travel as the raw body of `import.ts` (header with `options`).

import type { ImportSource, OpenApiApi, OpenApiOptions } from "@requesttailor/ui";

import type { Call } from "./backend";

/** The raw body of `openapi_inspect`/`openapi_plan`: a little-endian u32 with the length of a
 *  JSON header `{ options?, files: [{ name, size }] }`, the header, then the files' bytes. */
export function packSpec(files: ImportSource[], options: OpenApiOptions | null): Uint8Array {
  const header = new TextEncoder().encode(
    JSON.stringify({ options, files: files.map((f) => ({ name: f.name, size: f.data.byteLength })) }),
  );
  const body = new Uint8Array(4 + header.byteLength + files.reduce((n, f) => n + f.data.byteLength, 0));
  new DataView(body.buffer).setUint32(0, header.byteLength, true);
  body.set(header, 4);
  let at = 4 + header.byteLength;
  for (const f of files) {
    body.set(f.data, at);
    at += f.data.byteLength;
  }
  return body;
}

export function tauriOpenApi(call: Call): OpenApiApi {
  const raw = (body: Uint8Array) => body as unknown as Record<string, unknown>;
  return {
    fetch: (url) => call("openapi_fetch", { url }),
    // Tauri passes a Uint8Array argument through as the raw request body.
    inspect: (files) => call("openapi_inspect", raw(packSpec(files, null))),
    plan: (files, options) => call("openapi_plan", raw(packSpec(files, options))),
    requirements: (path, input) => call("openapi_requirements", { path, input: { environment: input.environment, vars: input.vars } }),
    validateResponse: (path, responseId) => call("openapi_validate_response", { path, responseId }),
  };
}
