// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// "Format body" for raw JSON and XML request bodies, with the same token-level formatters as
// the response's pretty view (placeholders stay byte-identical). Formatting is always an
// explicit edit: the shell's shortcut and palette entry ask the open request editor for it.

import type { Body } from "../commands/types";
import { formatJson } from "../pretty/json";
import { formatXml } from "../pretty/xml";

export type FormatResult = { kind: "formatted"; body: Body } | { kind: "unchanged" } | { kind: "malformed"; language: string };

/** Raw JSON and XML bodies can be formatted. */
export function canFormat(body: Body | undefined): boolean {
  return body?.type === "raw" && (body.kind === "json" || body.kind === "xml");
}

export function formatBody(body: Body): FormatResult | null {
  if (body.type !== "raw" || (body.kind !== "json" && body.kind !== "xml")) return null;
  const data = body.kind === "json" ? formatJson(body.data) : formatXml(body.data);
  if (data === null) return { kind: "malformed", language: body.kind === "json" ? "JSON" : "XML" };
  return data === body.data ? { kind: "unchanged" } : { kind: "formatted", body: { ...body, data } };
}

/** A sentence for the user, or `null` when the body changed (the editor shows that). */
export function describeFormat(result: FormatResult): string | null {
  switch (result.kind) {
    case "formatted":
      return null;
    case "unchanged":
      return "Already formatted";
    case "malformed":
      return `Not well-formed ${result.language}: nothing changed`;
  }
}

/** An open request editor, as the shell sees it. */
export interface FormatTarget {
  /** Its body is raw JSON or XML right now. */
  available(): boolean;
  format(): void;
}

const targets = new Set<FormatTarget>();

/** The open request editor formats its body when the shell asks (shortcut, palette). */
export function onFormatRequest(target: FormatTarget): () => void {
  targets.add(target);
  return () => targets.delete(target);
}

/** An open editor can format now. Otherwise the shortcut is not taken (the key types as usual). */
export function formatAvailable(): boolean {
  for (const t of targets) if (t.available()) return true;
  return false;
}

/** Formats the body of every open editor that can; false if none could. */
export function requestFormat(): boolean {
  let done = false;
  for (const t of targets) {
    if (!t.available()) continue;
    t.format();
    done = true;
  }
  return done;
}
