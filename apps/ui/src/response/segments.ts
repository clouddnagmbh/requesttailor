// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18 interface (strand J fills in the display): the time segments a send reported
// (`SendReport.segments`, from a plugin's `NotesReply.segments`), for the second line under the
// timing bar, "as reported by the server". They never count towards the total.

import type { SendReport, TimingSegment } from "../commands/types";

export function segmentsOf(report: SendReport): TimingSegment[] {
  return report.segments ?? [];
}

/** Wave 19: segments an extension contributed after the send (`ExtensionContext.segments`),
 *  per stored response and `source` (such as `"trace"`). A later call for the same source
 *  replaces the earlier one. They are shown in the line "as reported by <source>" under the
 *  timing bar and never count towards its total. Strand A shows them. */
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
