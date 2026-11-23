// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Where a spec comes from in the wizard's first step: files (one spec, a folder of a
// multi-file spec, a `.zip`), dropped or picked; pasted text; a URL (fetched by the backend
// over the core transport). Everything becomes `ImportSource[]` for `OpenApiApi.inspect`.

import type { ImportSource } from "../commands/backend";
import type { FetchedSpec } from "../commands/types";

const SPEC_EXT = /\.(ya?ml|json)$/i;

/** Reads picked or dropped files; a folder pick keeps the paths below the folder
 *  (`webkitRelativePath` without its first segment), so relative `$ref`s resolve. Only
 *  `.yaml`, `.yml`, `.json` and `.zip` files of a folder are read. */
export async function readSpecFiles(files: Iterable<File>): Promise<ImportSource[]> {
  const list = [...files];
  const fromFolder = list.some((f) => f.webkitRelativePath?.includes("/"));
  const out: ImportSource[] = [];
  for (const f of list) {
    let name = f.name;
    if (fromFolder && f.webkitRelativePath) {
      name = f.webkitRelativePath.split("/").slice(1).join("/") || f.name;
      if (!SPEC_EXT.test(name) || name.split("/").some((p) => p.startsWith("."))) continue;
    }
    out.push({ name, data: new Uint8Array(await f.arrayBuffer()) });
  }
  return out;
}

/** Pasted text as a file: JSON or YAML by its first character. */
export function pastedSpec(text: string): ImportSource {
  const json = /^\s*[{[]/.test(text);
  return { name: json ? "pasted.json" : "pasted.yaml", data: new TextEncoder().encode(text) };
}

export function fetchedSources(spec: FetchedSpec): ImportSource[] {
  const enc = new TextEncoder();
  return spec.files.map((f) => ({ name: f.name, data: enc.encode(f.text) }));
}

const decoder = new TextDecoder();

/** The first 4 KB look like an OpenAPI 3 or Swagger 2 document (YAML or JSON). */
export function looksLikeOpenApi(source: ImportSource): boolean {
  if (/\.zip$/i.test(source.name)) return true;
  const head = decoder.decode(source.data.subarray(0, 4096));
  return /^\s*["']?(openapi|swagger)["']?\s*:/m.test(head) || /"(openapi|swagger)"\s*:\s*"/.test(head);
}

/** A URL the wizard can fetch: http(s) with a host. */
export function validUrl(text: string): string | null {
  const t = text.trim();
  if (t === "") return null;
  try {
    const u = new URL(t);
    return u.protocol === "http:" || u.protocol === "https:" ? null : "Use an http:// or https:// address.";
  } catch {
    return "That is not a full address (https://…).";
  }
}

export const sizeText = (bytes: number) =>
  bytes < 1024 ? `${bytes} B` : bytes < 1024 * 1024 ? `${(bytes / 1024).toFixed(0)} KB` : `${(bytes / 1024 / 1024).toFixed(1)} MB`;
