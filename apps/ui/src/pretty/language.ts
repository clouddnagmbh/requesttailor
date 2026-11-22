// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The language of a body for highlighting and pretty printing: from the `Content-Type`, or
// sniffed from the first non-blank character when the type is missing or generic.

import { formatJson } from "./json";
import { formatXml } from "./xml";

export type BodyLanguage = "json" | "xml" | "html" | "javascript" | "css" | "text";

/** Types that say nothing about the text in them: the body decides. */
const GENERIC = new Set(["", "text/plain", "application/octet-stream", "*/*", "application/unknown"]);
const JAVASCRIPT = new Set([
  "application/javascript",
  "text/javascript",
  "application/x-javascript",
  "application/ecmascript",
  "text/ecmascript",
]);

/** The language a content type names; `null` if it is missing or generic (sniff the body). */
export function languageFromContentType(contentType: string | null): BodyLanguage | null {
  const mime = (contentType ?? "").split(";")[0].trim().toLowerCase();
  if (GENERIC.has(mime)) return null;
  const [type, subtype = ""] = mime.split("/");
  if (subtype === "json" || subtype === "x-json" || subtype.endsWith("+json")) return "json";
  if (mime === "text/html" || mime === "application/xhtml+xml") return "html";
  if (subtype === "xml" || subtype.endsWith("+xml")) return "xml";
  if (JAVASCRIPT.has(mime)) return "javascript";
  if (mime === "text/css") return "css";
  return type === "" ? null : "text";
}

/** Language from the start of the body: `{`/`[` JSON, `<` XML or HTML, else text. */
export function sniffLanguage(head: string): BodyLanguage {
  const m = /^﻿?\s*(\S)/.exec(head);
  if (!m) return "text";
  if (m[1] === "{" || m[1] === "[") return "json";
  if (m[1] !== "<") return "text";
  const start = head.slice(m.index + m[0].length - 1, m.index + m[0].length + 32);
  return /^<(?:!doctype\s+html|html[\s>])/i.test(start) ? "html" : "xml";
}

export function bodyLanguage(contentType: string | null, head: string): BodyLanguage {
  return languageFromContentType(contentType) ?? sniffLanguage(head);
}

/**
 * The text for the pretty view: JSON and XML re-indented (if well-formed), everything else
 * as it is. HTML is never reflowed (`<pre>`, `<script>`, `<textarea>` and inline whitespace
 * matter there).
 */
export function prettyBody(text: string, lang: BodyLanguage): { text: string; formatted: boolean } {
  const pretty = lang === "json" ? formatJson(text) : lang === "xml" ? formatXml(text) : null;
  return pretty === null ? { text, formatted: false } : { text: pretty, formatted: true };
}
