// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The layout of a sequence diagram (wave 19, strand G): where every participant, message, note
// and group goes. Pure and deterministic (the same model gives the same numbers in the desktop
// app and in the browser), no DOM, no library. The view draws only what is in the window
// (`visibleItems`), so a diagram of 2 000 messages scrolls like a short one.

import type { SequenceMessage, SequenceModel, SequenceParticipant } from "../commands/types";

export const COLUMN_WIDTH = 200;
export const MARGIN = 24;
export const ROW_HEIGHT = 36;
export const NOTE_HEIGHT = 28;
export const GROUP_INSET = 6;
/** Pixels one character of the 12 px label font takes, a little generously. */
export const CHAR_WIDTH = 6.8;

export interface Column {
  participant: SequenceParticipant;
  /** The centre line. */
  x: number;
}

export interface LaidMessage {
  type: "message";
  index: number;
  y: number;
  height: number;
  /** Where the arrow starts and ends. */
  x1: number;
  x2: number;
  /** Both ends on one participant: drawn as a small loop to the right. */
  self: boolean;
  kind: SequenceMessage["kind"];
  label: string;
  requestPath?: string;
}

export interface LaidNote {
  type: "note";
  /** The message the note follows. */
  after: number;
  y: number;
  height: number;
  x: number;
  width: number;
  text: string;
}

export interface LaidGroup {
  kind: string;
  label: string;
  depth: number;
  y1: number;
  y2: number;
  x1: number;
  x2: number;
}

export interface Layout {
  width: number;
  height: number;
  columns: Column[];
  /** Messages and notes, by `y`. */
  items: (LaidMessage | LaidNote)[];
  groups: LaidGroup[];
  /** The `y` of every message (the rows keyboard and focus move between). */
  messageY: number[];
}

/** The width a text needs at the label size, between a floor and a ceiling. */
export function textWidth(text: string, min: number, max: number): number {
  return Math.max(min, Math.min(max, text.length * CHAR_WIDTH + 16));
}

export function layoutSequence(model: SequenceModel, columnWidth = COLUMN_WIDTH): Layout {
  const columns: Column[] = model.participants.map((participant, i) => ({
    participant,
    x: MARGIN + columnWidth * i + columnWidth / 2,
  }));
  const width = MARGIN * 2 + columnWidth * Math.max(1, columns.length);
  const xOf = new Map(columns.map((c) => [c.participant.id, c.x]));
  const x = (id: string): number => xOf.get(id) ?? columns[0]?.x ?? MARGIN;

  const notes = new Map<number, NonNullable<SequenceModel["notes"]>>();
  for (const n of model.notes ?? []) {
    const list = notes.get(n.after);
    if (list) list.push(n);
    else notes.set(n.after, [n]);
  }

  const items: (LaidMessage | LaidNote)[] = [];
  const messageY: number[] = [];
  /** `y` after the last row of each message and its notes. */
  const blockEnd: number[] = [];
  let y = 0;
  model.messages.forEach((m, index) => {
    messageY.push(y);
    const x1 = x(m.from);
    const x2 = x(m.to);
    items.push({
      type: "message",
      index,
      y,
      height: ROW_HEIGHT,
      x1,
      x2,
      self: m.from === m.to,
      kind: m.kind,
      label: m.label,
      requestPath: m.request_path,
    });
    y += ROW_HEIGHT;
    for (const note of notes.get(index) ?? []) {
      let left: number;
      let w: number;
      if (note.anchor.kind === "over") {
        const cx = x(note.anchor.participant);
        w = textWidth(note.text, 80, columnWidth * 1.8);
        left = Math.max(4, Math.min(width - w - 4, cx - w / 2));
      } else {
        const a = x(note.anchor.from);
        const b = x(note.anchor.to);
        const span = Math.abs(a - b) + 40;
        w = Math.max(span, textWidth(note.text, 80, columnWidth * 1.8));
        left = Math.max(4, Math.min(width - w - 4, (a + b) / 2 - w / 2));
      }
      items.push({ type: "note", after: index, y, height: NOTE_HEIGHT, x: left, width: w, text: note.text });
      y += NOTE_HEIGHT;
    }
    blockEnd.push(y);
  });

  // Groups, outer first at the same start, nested by a stack.
  const groups: LaidGroup[] = [];
  const sorted = [...(model.groups ?? [])]
    .filter((g) => g.from <= g.to && g.to < model.messages.length)
    .sort((a, b) => a.from - b.from || b.to - a.to);
  const open: number[] = [];
  for (const g of sorted) {
    while (open.length > 0 && (open[open.length - 1] ?? -1) < g.from) open.pop();
    const depth = open.length;
    open.push(g.to);
    groups.push({
      kind: g.kind,
      label: g.label,
      depth,
      y1: messageY[g.from] ?? 0,
      y2: blockEnd[g.to] ?? y,
      x1: MARGIN / 2 + depth * GROUP_INSET,
      x2: width - MARGIN / 2 - depth * GROUP_INSET,
    });
  }
  return { width, height: y + ROW_HEIGHT / 2, columns, items, groups, messageY };
}

/** The items that intersect `[top, top + height)`, by binary search (items are sorted by `y`). */
export function visibleItems(layout: Layout, top: number, height: number, overscan = 120): (LaidMessage | LaidNote)[] {
  const from = top - overscan;
  const to = top + height + overscan;
  const items = layout.items;
  let lo = 0;
  let hi = items.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    const item = items[mid];
    if (item && item.y + item.height < from) lo = mid + 1;
    else hi = mid;
  }
  const out: (LaidMessage | LaidNote)[] = [];
  for (let i = lo; i < items.length; i++) {
    const item = items[i];
    if (!item || item.y > to) break;
    out.push(item);
  }
  return out;
}

export function visibleGroups(layout: Layout, top: number, height: number): LaidGroup[] {
  return layout.groups.filter((g) => g.y2 >= top - 40 && g.y1 <= top + height + 40);
}

/** The label that fits between two lifelines (cut with an ellipsis; the full text is a title). */
export function fitLabel(label: string, pixels: number): string {
  const max = Math.max(4, Math.floor((pixels - 12) / CHAR_WIDTH));
  return label.length <= max ? label : `${label.slice(0, max - 1)}…`;
}
