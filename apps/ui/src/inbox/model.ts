// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Pure parts of the webhook inbox view (wave 18, strand E): applying events to the list, the
// search, the one-line summary, the visible window of the virtualized list and what "Make
// request" proposes. No rule about secrets lives here: the draft comes from the core
// (`InboxApi.makeRequest`, `rt_app::inbox::draft`).

import type { CapturedRequest, InboxDraft, InboxEvent, InboxSettings, NewRequest, PluginEnvironment } from "../commands/types";
import { formatJson } from "../pretty/json";
import { languageFromContentType, sniffLanguage, type BodyLanguage } from "../pretty/language";

/** The most the list shows (the same as the buffer of the core). */
export const MAX_SHOWN = 500;

/** Height of a row of the list in pixels (the list is virtualized). */
export const ROW_HEIGHT = 32;

/**
 * Applies events to the list (oldest first). `cleared` empties it. `dropped` changes nothing here:
 * a page of events already leaves out the requests that fell out, so the oldest are removed by
 * [`trimTo`] with the number the status says is held.
 */
export function applyEvents(list: CapturedRequest[], events: InboxEvent[]): CapturedRequest[] {
  let out = list;
  for (const event of events) {
    switch (event.kind) {
      case "request":
        if (!out.some((r) => r.id === event.request.id)) out = [...out, event.request];
        break;
      case "cleared":
        out = [];
        break;
      default:
        break;
    }
  }
  return out.length > MAX_SHOWN ? out.slice(out.length - MAX_SHOWN) : out;
}

/** The newest `held` requests: what the inbox still has (older ones fell out of its buffer). */
export function trimTo(list: CapturedRequest[], held: number): CapturedRequest[] {
  return list.length > held ? list.slice(list.length - held) : list;
}

/** `POST /hooks/orders?x=1`. */
export function summary(r: CapturedRequest): string {
  return `${r.method} ${r.target}`;
}

/** Case-insensitive search over method, target, header names and values, and the body. */
export function matches(r: CapturedRequest, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (q === "") return true;
  if (summary(r).toLowerCase().includes(q)) return true;
  if (r.headers.some(([n, v]) => n.toLowerCase().includes(q) || v.toLowerCase().includes(q))) return true;
  return (r.body ?? "").toLowerCase().includes(q);
}

export function contentTypeOf(r: CapturedRequest): string | null {
  return r.headers.find(([n]) => n.toLowerCase() === "content-type")?.[1] ?? null;
}

/** The body as shown (JSON pretty printed when it parses) and its language. */
export function bodyView(r: CapturedRequest): { text: string; lang: BodyLanguage } | null {
  if (r.body === null || r.body === "") return null;
  const lang = languageFromContentType(contentTypeOf(r)) ?? sniffLanguage(r.body.slice(0, 256));
  if (lang === "json") {
    const pretty = formatJson(r.body);
    if (pretty !== null) return { text: pretty, lang };
  }
  return { text: r.body, lang };
}

/** `14:03:22` in the local time zone. */
export function clock(at: number): string {
  const d = new Date(at);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

export function sizeLabel(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/** The rows to render: indexes `[from, to)` for the scroll position, with `overscan` rows each side. */
export function visibleRange(scrollTop: number, viewport: number, count: number, overscan = 6): [number, number] {
  const first = Math.floor(scrollTop / ROW_HEIGHT);
  const rows = Math.ceil(viewport / ROW_HEIGHT);
  return [Math.max(0, first - overscan), Math.min(count, first + rows + overscan)];
}

/** `http://127.0.0.1:47855/` + `hooks/orders` for "Copy URL" of a path; the base alone without. */
export function copyUrl(status: { url?: string }): string {
  return status.url ?? "";
}

/** Settings from the form, with the numbers clamped the way the core accepts them. */
export function cleanSettings(s: InboxSettings): InboxSettings {
  const whole = (n: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, Math.round(Number.isFinite(n) ? n : lo)));
  const contentType = s.content_type?.trim();
  return {
    port: whole(s.port, 0, 65535),
    status: whole(s.status, 200, 599),
    body: s.body,
    delay_ms: whole(s.delay_ms, 0, 5000),
    ...(contentType ? { content_type: contentType } : {}),
  };
}

/**
 * What "Make request" proposes: the request in the collection root and, when an environment is
 * selected, its secret variables without a value (the user sets them; they go to the key store).
 * `AdditionsDialog` shows it and applies it unsaved.
 */
export function proposal(draft: InboxDraft, environment: string | null): {
  create_requests: NewRequest[];
  add_environments: PluginEnvironment[];
} {
  const create_requests: NewRequest[] = [{ folder: "", name: draft.name, spec: draft.spec }];
  const add_environments: PluginEnvironment[] =
    environment && draft.secrets.length > 0
      ? [{ name: environment, variables: draft.secrets.map((name) => ({ name, value: "", secret: true })) }]
      : [];
  return { create_requests, add_environments };
}
