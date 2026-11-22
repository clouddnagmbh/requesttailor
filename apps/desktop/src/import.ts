// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ImportApi` of the desktop app (src-tauri/src/import.rs). Owner: wave 4, strand B.

import type { ImportApi, ImportFormat, ImportSource } from "@requesttailor/ui";

import type { Call } from "./backend";

/** The raw IPC body of `import_analyze`: a little-endian u32 with the length of a JSON header
 *  `{ format, files: [{ name, size }] }`, the header, then the files' bytes in order. Raw bytes
 *  instead of JSON number arrays keep a multi-megabyte export fast and small. */
export function packImport(files: ImportSource[], format: ImportFormat | null): Uint8Array {
  return pack(files, { format });
}

/** `packImport` for `import_analyze_plugin`: the header has `plugin` and `importer`. */
export function packPluginImport(files: ImportSource[], plugin: string, importer: string): Uint8Array {
  return pack(files, { plugin, importer });
}

function pack(files: ImportSource[], head: Record<string, unknown>): Uint8Array {
  const header = new TextEncoder().encode(
    JSON.stringify({ ...head, files: files.map((f) => ({ name: f.name, size: f.data.byteLength })) }),
  );
  const total = 4 + header.byteLength + files.reduce((n, f) => n + f.data.byteLength, 0);
  const body = new Uint8Array(total);
  new DataView(body.buffer).setUint32(0, header.byteLength, true);
  body.set(header, 4);
  let at = 4 + header.byteLength;
  for (const f of files) {
    body.set(f.data, at);
    at += f.data.byteLength;
  }
  return body;
}

export function tauriImports(call: Call): ImportApi {
  return {
    // Tauri passes a Uint8Array argument through as the raw request body.
    analyze: (files, format) =>
      call("import_analyze", packImport(files, format) as unknown as Record<string, unknown>),
    // Same raw body as `import_analyze`; the header names the plugin and its importer.
    analyzeWithPlugin: (files, plugin, importer) =>
      call("import_analyze_plugin", packPluginImport(files, plugin, importer) as unknown as Record<string, unknown>),
    write: (id) => call("import_write", { id }),
    discard: (id) => call("import_discard", { id }),
    // Wave 13: a collection converted by an extension; into the open collection as a folder.
    adopt: (imported) => call("import_adopt", { import: imported }),
    writeInto: (id, name) => call("import_write_into", { id, name: name ?? null }),
  };
}
