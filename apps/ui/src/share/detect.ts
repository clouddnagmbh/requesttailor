// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// What a pasted text is (wave 17, strand C): a cURL command, or a link, a fragment or an
// `rt:` payload of this app. Mirrors `rt_app::share::find_payload` and
// `rt_app::ai::snippet::is_curl`; the core decides when it reads them, this only decides
// whether to offer. No request is made.

import { looksLikeCurl } from "../ai/task";

export type PasteKind = "curl" | "share";

/** The longest paste looked at. */
export const MAX_PASTE = 128 * 1024;

const PAYLOAD = /^[0-9]+\.[A-Za-z0-9_-]+$/;

/** Whether `text` is a link, fragment (`#rt=…`) or `rt:` payload (`<digits>.<base64url>`). */
export function looksShared(text: string): boolean {
  const t = text.trim();
  if (t.length === 0 || t.length > MAX_PASTE) return false;
  if (t.startsWith("rt:")) return PAYLOAD.test(t.slice(3).split(/\s+/)[0] ?? "");
  const hash = t.indexOf("#");
  const fragment = hash >= 0 ? t.slice(hash + 1) : t.startsWith("rt=") ? t : null;
  if (fragment === null) return false;
  return fragment.split("&").some((p) => p.startsWith("rt=") && PAYLOAD.test(p.slice(3).split(/\s+/)[0] ?? ""));
}

/** What the pasted text is, if it is something this app can open as a request. */
export function classifyPaste(text: string): PasteKind | null {
  if (text.length > MAX_PASTE) return null;
  if (looksLikeCurl(text)) return "curl";
  if (looksShared(text)) return "share";
  return null;
}
