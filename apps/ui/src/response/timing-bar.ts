// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The one-line timing bar (wave 17, strand D): the phases of `waterfall()` stacked in a single
// bar, with the sentence "Server wait is 82 % of 410 ms". Pure evaluation of the report's
// `Timing` and total; no core type changes. The text twin on the command line is
// `rt send --timing` (`crates/rt-cli/src/send_banner.rs`).

import type { TransportMode } from "../commands/backend";
import type { Timing } from "../commands/types";
import { formatDuration, waterfall, type PhaseName } from "./format";

export type BarPhase = "dns" | "connect" | "tls" | "wait" | "transfer";

export interface BarSegment {
  name: BarPhase;
  label: string;
  ms: number;
  /** Share of the total, 0 to 100. */
  pct: number;
}

export interface TimingBarData {
  segments: BarSegment[];
  total: number;
  /** "Server wait is 82 % of 410 ms". */
  sentence: string;
  /** Phases the transport did not see, with the reason. */
  hidden: { name: PhaseName; label: string; reason: string }[];
  /** Tooltip: every segment with its time, and what is not shown. */
  title: string;
}

const LABELS: Record<BarPhase, string> = {
  dns: "DNS",
  connect: "Connect",
  tls: "TLS",
  wait: "Server wait",
  transfer: "Transfer",
};

const BAR_NAME: Record<string, BarPhase> = { dns: "dns", connect: "connect", tls: "tls", ttfb: "wait", download: "transfer" };

export function timingBar(timing: Timing, total: number, mode: TransportMode, url: string): TimingBarData {
  const w = waterfall(timing, total, mode, url);
  const segments: BarSegment[] = w.phases.map((p) => {
    const name = BAR_NAME[p.name]!;
    return { name, label: LABELS[name], ms: p.duration, pct: w.total > 0 ? (p.duration / w.total) * 100 : 0 };
  });
  const wait = segments.find((s) => s.name === "wait")!;
  const sentence = `Server wait is ${Math.round(wait.pct)} % of ${formatDuration(w.total)}`;
  const parts = segments.map((s) => `${s.label} ${formatDuration(s.ms)}`);
  const hidden = w.hidden.map((h) => `${h.label} (${h.reason})`);
  const title = hidden.length > 0 ? `${parts.join(" · ")}\nNot shown: ${hidden.join("; ")}` : parts.join(" · ");
  return { segments, total: w.total, sentence, hidden: w.hidden, title };
}

/** A history row has only the duration: one bar against the slowest row of the list. */
export function durationBar(ms: number | null, slowest: number): { pct: number; label: string } | null {
  if (ms === null) return null;
  const pct = slowest > 0 ? Math.min(100, (ms / slowest) * 100) : 100;
  return { pct, label: formatDuration(ms) };
}
