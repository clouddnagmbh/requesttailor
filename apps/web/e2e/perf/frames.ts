// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Frame-time measurement while scrolling a response (budget `large_response_frame`,
// perf/README.md). The scroll is driven from inside the page, one step per animation frame,
// so every frame has work to do; the frame time is the interval between consecutive
// `requestAnimationFrame` callbacks.

import { writeFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";

import type { Locator, Page } from "@playwright/test";

export interface Phase {
  name: string;
  /** Frames measured in this phase. */
  frames: number;
  p50: number;
  p95: number;
  max: number;
}

export interface ScrollRun {
  phases: Phase[];
  /** All intervals of all phases, sorted (ms). */
  intervals: number[];
  p95: number;
  /** Intervals longer than 1.5 refresh periods (a frame was dropped). */
  dropped: number;
  /** Median interval of an idle page before scrolling: the display's refresh period. */
  refreshMs: number;
}

export function percentile(sorted: number[], p: number): number {
  if (sorted.length === 0) return NaN;
  const i = Math.min(sorted.length - 1, Math.max(0, Math.ceil((p / 100) * sorted.length) - 1));
  return sorted[i];
}

const round = (n: number) => Math.round(n * 100) / 100;

/** Scrolls the virtual list `viewport` (`data-testid="response-lines"`) in three phases and
 *  measures every frame:
 *  - `wheel`: a fast, steady wheel scroll (`pxPerFrame` per frame, synthetic wheel events
 *    handled by the list's own wheel handler),
 *  - `jump`: a jump to a random place in the whole body every frame (dragging the scrollbar
 *    thumb across 100 MB; every frame needs lines that are not cached),
 *  - `keys`: PageDown every frame (keyboard scrolling). */
export async function measureScroll(page: Page, viewport: Locator, framesPerPhase = 240, pxPerFrame = 600): Promise<ScrollRun> {
  await viewport.focus();
  const handle = await viewport.elementHandle();
  if (!handle) throw new Error("response viewport not found");
  const raw = await page.evaluate(
    async ({ el, framesPerPhase, pxPerFrame }) => {
      const frame = () => new Promise<number>((r) => requestAnimationFrame(r));
      // Refresh period of an idle page.
      const idle: number[] = [];
      let last = await frame();
      for (let i = 0; i < 60; i++) {
        const t = await frame();
        idle.push(t - last);
        last = t;
      }
      const track = el.parentElement!.querySelector<HTMLElement>('[data-testid="response-scrollbar"]')!;
      const lines = Number(track.getAttribute("aria-valuemax")) + 1;
      const rowPx = 18;
      // Deterministic "random" positions (no Math.random: runs are comparable).
      let seed = 42;
      const next = () => (seed = (seed * 1103515245 + 12345) % 2 ** 31) / 2 ** 31;
      const wheel = (deltaY: number) =>
        el.dispatchEvent(new WheelEvent("wheel", { deltaY, deltaMode: 0, bubbles: true, cancelable: true }));
      const key = (k: string) => el.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true }));
      const steps: Record<string, () => void> = {
        wheel: () => wheel(pxPerFrame),
        // Home, then a wheel delta to a random line: the frame shows that line.
        jump: () => {
          key("Home");
          wheel(Math.floor(next() * lines) * rowPx);
        },
        keys: () => key("PageDown"),
      };
      const phases: { name: string; intervals: number[] }[] = [];
      for (const [name, step] of Object.entries(steps)) {
        // Start of each phase at the top, after a settled frame.
        key("Home");
        await frame();
        last = await frame();
        const intervals: number[] = [];
        for (let i = 0; i < framesPerPhase; i++) {
          step();
          const t = await frame();
          intervals.push(t - last);
          last = t;
        }
        phases.push({ name, intervals });
      }
      return { idle, phases };
    },
    { el: handle as unknown as HTMLElement, framesPerPhase, pxPerFrame },
  );

  const sort = (a: number[]) => [...a].sort((x, y) => x - y);
  const refreshMs = percentile(sort(raw.idle), 50);
  const phases: Phase[] = raw.phases.map(({ name, intervals }) => {
    const s = sort(intervals);
    return { name, frames: s.length, p50: round(percentile(s, 50)), p95: round(percentile(s, 95)), max: round(s[s.length - 1]) };
  });
  const intervals = sort(raw.phases.flatMap((p) => p.intervals));
  return {
    phases,
    intervals,
    p95: round(percentile(intervals, 95)),
    dropped: intervals.filter((d) => d > refreshMs * 1.5).length,
    refreshMs: round(refreshMs),
  };
}

/** Negative control: the same frame-interval method over frames that each burn `busyMs` of
 *  main-thread time. Returns the p95 interval; it must show the stutter (about two refresh
 *  periods), or the method would not detect dropped frames. */
export async function measureControl(page: Page, busyMs = 25, frames = 60): Promise<number> {
  const intervals = await page.evaluate(
    async ({ busyMs, frames }) => {
      const frame = () => new Promise<number>((r) => requestAnimationFrame(r));
      const out: number[] = [];
      let last = await frame();
      for (let i = 0; i < frames; i++) {
        const until = performance.now() + busyMs;
        while (performance.now() < until) {
          // Busy: the main thread cannot produce the next frame on time.
        }
        const t = await frame();
        out.push(t - last);
        last = t;
      }
      return out;
    },
    { busyMs, frames },
  );
  return percentile(
    intervals.sort((a, b) => a - b),
    95,
  );
}

/** Writes `value` as `<RT_PERF_OUT>/<name>.json` when RT_PERF_OUT is set (scripts/perf/run.mjs). */
export function writePerfResult(name: string, value: unknown): void {
  const dir = process.env.RT_PERF_OUT;
  if (!dir) return;
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, `${name}.json`), `${JSON.stringify(value, null, 2)}\n`);
}
