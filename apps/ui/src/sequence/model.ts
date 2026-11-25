// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Plain helpers of the sequence view (wave 19, strand G): which sources the view offers, what the
// keyboard does, what the screen reader hears, the file name of a saved diagram.

import type { SequenceModel, SequenceSource } from "../commands/types";

export interface SourceChoice {
  id: "run" | "session" | "pages" | "mcp" | "history";
  label: string;
  source: SequenceSource;
}

/** The sources the view offers; the pages of one request only with a request in hand. */
export function sourceChoices(activeRequest: string | null): SourceChoice[] {
  const out: SourceChoice[] = [
    { id: "run", label: "Latest run", source: { kind: "run", run_id: 0 } },
    { id: "session", label: "This session", source: { kind: "session" } },
  ];
  if (activeRequest) out.push({ id: "pages", label: "Pages of this request", source: { kind: "pages", request_path: activeRequest } });
  out.push({ id: "mcp", label: "Agent (MCP) runs", source: { kind: "mcp", since: null } });
  out.push({ id: "history", label: "History", source: { kind: "history", ids: [] } });
  return out;
}

/** The message `delta` rows from `from` (clamped); with nothing selected, the first or last. */
export function moveSelection(from: number | null, delta: number, count: number): number | null {
  if (count === 0) return null;
  if (from === null) return delta >= 0 ? 0 : count - 1;
  return Math.max(0, Math.min(count - 1, from + delta));
}

/** What a screen reader says for the selected message. */
export function describeSelection(model: SequenceModel, index: number | null): string {
  if (index === null) return `${model.messages.length} messages. Use the arrow keys to move between them.`;
  const m = model.messages[index];
  if (!m) return "";
  const name = (id: string): string => model.participants.find((p) => p.id === id)?.label ?? id;
  const open = m.request_path ? " Enter opens the request." : "";
  return `Message ${index + 1} of ${model.messages.length}: ${m.kind} from ${name(m.from)} to ${name(m.to)}, ${m.label}.${open}`;
}

/** `sequence/<stem>-<yyyymmdd-hhmmss>.md`, for "Save as .md" (a companion file of the collection). */
export function savedName(stem: string, now: Date): string {
  const p = (n: number): string => String(n).padStart(2, "0");
  const stamp = `${now.getFullYear()}${p(now.getMonth() + 1)}${p(now.getDate())}-${p(now.getHours())}${p(now.getMinutes())}${p(now.getSeconds())}`;
  return `sequence/${stem.replace(/[^a-z0-9-]+/gi, "-").toLowerCase()}-${stamp}.md`;
}

/** The sentence under the diagram when messages were left out. */
export function omittedText(model: SequenceModel): string | null {
  const n = model.omitted ?? 0;
  return n > 0 ? `${n} more messages are not shown (the diagram holds at most 2 000).` : null;
}
