// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18 interface (strand J fills in the display): the time segments a send reported
// (`SendReport.segments`, from a plugin's `NotesReply.segments`), for the second line under the
// timing bar, "as reported by the server". They never count towards the total.

import type { SendReport, TimingSegment } from "../commands/types";
import { formatDuration } from "./format";

export function segmentsOf(report: SendReport): TimingSegment[] {
  return report.segments ?? [];
}

/** Wave 19: segments an extension contributed after the send (`ExtensionContext.segments`),
 *  per stored response and `source` (such as `"trace"`). A later call for the same source
 *  replaces the earlier one. They are shown as "as reported by <source>" under the
 *  waterfall (`trace/TraceSegments.svelte`) and never count towards its total. */
export interface SegmentSet {
  source: string;
  segments: TimingSegment[];
}

export class SegmentContributions {
  private readonly byResponse = new Map<number, Map<string, TimingSegment[]>>();
  private readonly listeners = new Set<() => void>();

  contribute(responseId: number, source: string, segments: TimingSegment[]): void {
    let sources = this.byResponse.get(responseId);
    if (!sources) this.byResponse.set(responseId, (sources = new Map()));
    if (segments.length === 0) sources.delete(source);
    else sources.set(source, segments);
    for (const listener of this.listeners) listener();
  }

  forResponse(responseId: number): SegmentSet[] {
    return [...(this.byResponse.get(responseId) ?? [])].map(([source, segments]) => ({ source, segments }));
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }
}

// ---- the tree (wave 19, strand A) ----

/** One line of a laid-out segment tree. Times are on the server's own clock, relative to the
 *  start of the first segment; never on the client's bar. */
export interface SegmentRow {
  segment: TimingSegment;
  depth: number;
  /** Milliseconds after the server began (the first span). */
  start: number;
  end: number;
}

export interface SegmentLayout {
  rows: SegmentRow[];
  /** The length of the whole picture (latest end, at least 1 ms), the scale of the bars. */
  span: number;
  /** Longest end of the segments that hang off the server wait or off nothing. */
  top: number;
}

/** Lays segments out as a tree. A `parent` that is the id of another segment of the set makes
 *  a child (`"server"`, `undefined` or an unknown id make a top-level segment; the members of a cycle
 *  become top-level). Children follow their parent in input order. A segment
 *  without `offset_ms` starts where its previous sibling ended, or at its parent's start. */
export function layoutSegments(segments: TimingSegment[]): SegmentLayout {
  const byId = new Map<string, TimingSegment>();
  for (const s of segments) if (!byId.has(s.id)) byId.set(s.id, s);
  const parentOf = (s: TimingSegment): string | null => {
    const p = s.parent;
    if (p === undefined || p === s.id || !byId.has(p)) return null;
    // A chain that comes back to `s` is a cycle: its members all become top-level.
    const seen = new Set<string>([s.id]);
    for (let cur: string | undefined = p; cur !== undefined && byId.has(cur); cur = byId.get(cur)!.parent) {
      if (cur === s.id) return null;
      if (seen.has(cur)) break;
      seen.add(cur);
    }
    return p;
  };
  const children = new Map<string | null, TimingSegment[]>();
  const placed = new Set<TimingSegment>();
  for (const s of byId.values()) {
    const key = parentOf(s);
    const list = children.get(key) ?? [];
    list.push(s);
    children.set(key, list);
  }
  const rows: SegmentRow[] = [];
  const visit = (key: string | null, depth: number, from: number, guard: Set<string>) => {
    let cursor = from;
    for (const s of children.get(key) ?? []) {
      if (placed.has(s)) continue;
      placed.add(s);
      const start = Math.max(0, s.offset_ms ?? cursor);
      const end = start + s.ms;
      rows.push({ segment: s, depth, start, end });
      cursor = end;
      if (!guard.has(s.id)) {
        guard.add(s.id);
        visit(s.id, depth + 1, start, guard);
        guard.delete(s.id);
      }
    }
  };
  visit(null, 0, 0, new Set());
  // Anything left sits in a cycle that parentOf could not cut: show it at the top.
  for (const s of byId.values()) {
    if (!placed.has(s)) {
      placed.add(s);
      const start = Math.max(0, s.offset_ms ?? 0);
      rows.push({ segment: s, depth: 0, start, end: start + s.ms });
    }
  }
  const span = Math.max(1, ...rows.map((r) => r.end));
  const top = Math.max(0, ...rows.filter((r) => r.depth === 0).map((r) => r.end));
  return { rows, span, top };
}

/** The hint when the top-level segments run longer than the wait for the server. */
export function overWaitHint(layout: SegmentLayout, waitMs: number): string | null {
  if (layout.top <= waitMs) return null;
  return `Longer than the ${formatDuration(waitMs)} wait for the server: its clock is not ours, so the numbers stay as reported.`;
}
