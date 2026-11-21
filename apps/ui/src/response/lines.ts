// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Line sources and the chunk cache behind the virtual line list. A source is anything that
// can hand out a range of lines: the backend's stored body (`responseLines`), a local array
// (pretty-printed JSON, filter results) or a hex dump over `responseBytes`.

import type { LineSlice } from "../commands/types";

export interface LineSource {
  /** Number of lines. */
  readonly count: number;
  get(start: number, count: number): Promise<LineSlice>;
}

export interface Line {
  text: string;
  /** Cut by the backend at its line length limit. */
  cut: boolean;
}

/** Lines of an in-memory array. */
export function arraySource(lines: string[]): LineSource {
  return {
    count: lines.length,
    get: async (start, count) => ({ start, lines: lines.slice(start, start + count), cut: [] }),
  };
}

export const HEX_BYTES_PER_LINE = 16;

/** One hex dump line: offset, 16 bytes in hex, printable ASCII. */
export function hexLine(offset: number, bytes: Uint8Array): string {
  let hex = "";
  let ascii = "";
  for (let i = 0; i < HEX_BYTES_PER_LINE; i++) {
    if (i === 8) hex += " ";
    if (i < bytes.length) {
      const b = bytes[i];
      hex += b.toString(16).padStart(2, "0") + " ";
      ascii += b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : ".";
    } else {
      hex += "   ";
    }
  }
  return `${offset.toString(16).padStart(8, "0")}  ${hex} ${ascii}`;
}

/** Hex dump over `bytes(offset, length)`. */
export function hexSource(
  size: number,
  bytes: (offset: number, length: number) => Promise<Uint8Array>,
): LineSource {
  return {
    count: Math.ceil(size / HEX_BYTES_PER_LINE),
    async get(start, count) {
      const data = await bytes(start * HEX_BYTES_PER_LINE, count * HEX_BYTES_PER_LINE);
      const lines: string[] = [];
      for (let k = 0; k * HEX_BYTES_PER_LINE < data.length; k++) {
        const off = k * HEX_BYTES_PER_LINE;
        lines.push(hexLine((start + k) * HEX_BYTES_PER_LINE, data.subarray(off, off + HEX_BYTES_PER_LINE)));
      }
      return { start, lines, cut: [] };
    },
  };
}

/**
 * Fixed-size chunks of lines, fetched on demand and evicted least recently used first, so
 * memory stays bounded however far the user scrolls.
 */
export class LineCache {
  private chunks = new Map<number, Line[]>();
  private pending = new Map<number, Promise<void>>();
  private disposed = false;

  constructor(
    private readonly source: LineSource,
    /** Called whenever a chunk arrived (to re-render). */
    private readonly onLoad: () => void = () => {},
    readonly chunkSize = 256,
    readonly maxChunks = 64,
  ) {}

  get count(): number {
    return this.source.count;
  }

  /** Number of cached chunks (for tests). */
  get size(): number {
    return this.chunks.size;
  }

  /** The line if its chunk is cached. `ensure` marks chunks as recently used. */
  get(i: number): Line | undefined {
    const c = Math.floor(i / this.chunkSize);
    return this.chunks.get(c)?.[i - c * this.chunkSize];
  }

  /** Fetches the chunks covering lines `first..=last` that are neither cached nor pending. */
  ensure(first: number, last: number): Promise<void> {
    first = Math.max(0, first);
    last = Math.min(this.count - 1, last);
    const wanted: Promise<void>[] = [];
    for (let c = Math.floor(first / this.chunkSize); c <= Math.floor(last / this.chunkSize); c++) {
      if (this.chunks.has(c)) {
        const chunk = this.chunks.get(c)!;
        this.chunks.delete(c);
        this.chunks.set(c, chunk);
        continue;
      }
      wanted.push(this.pending.get(c) ?? this.load(c));
    }
    return Promise.all(wanted).then(() => undefined);
  }

  dispose(): void {
    this.disposed = true;
    this.chunks.clear();
  }

  private load(c: number): Promise<void> {
    const start = c * this.chunkSize;
    const count = Math.min(this.chunkSize, this.count - start);
    const p = this.source
      .get(start, count)
      .then((slice) => {
        if (this.disposed) return;
        const cut = new Set(slice.cut);
        this.chunks.set(
          c,
          slice.lines.map((text, k) => ({ text, cut: cut.has(k) })),
        );
        this.evict();
        this.onLoad();
      })
      .finally(() => this.pending.delete(c));
    this.pending.set(c, p);
    return p;
  }

  private evict(): void {
    // Map iteration order is insertion order: the first keys are the least recently used.
    for (const key of this.chunks.keys()) {
      if (this.chunks.size <= this.maxChunks) break;
      this.chunks.delete(key);
    }
  }
}

/** Vertical position model of the virtual list, in virtual pixels (not DOM pixels). */
export interface ScrollModel {
  count: number;
  rowHeight: number;
  viewport: number;
}

export function maxTop(m: ScrollModel): number {
  return Math.max(0, m.count * m.rowHeight - m.viewport);
}

export function clampTop(m: ScrollModel, top: number): number {
  return Math.min(maxTop(m), Math.max(0, Number.isFinite(top) ? top : 0));
}

/** First visible line and how many rows to render to cover the viewport. */
export function visibleRange(m: ScrollModel, top: number): { first: number; rows: number } {
  const first = Math.min(Math.max(0, m.count - 1), Math.floor(top / m.rowHeight));
  const rows = Math.max(0, Math.min(m.count - first, Math.ceil(m.viewport / m.rowHeight) + 1));
  return { first, rows };
}

export const MIN_THUMB = 24;

/**
 * Scrollbar thumb for a track of `track` DOM pixels. The content height can exceed what the
 * browser can lay out (around 16-33 million pixels), so the thumb maps the whole virtual
 * range onto the track instead of relying on native scrolling.
 */
export function thumb(m: ScrollModel, track: number, top: number): { size: number; offset: number } {
  const total = m.count * m.rowHeight;
  if (total <= m.viewport || track <= 0) return { size: track, offset: 0 };
  const size = Math.min(track, Math.max(MIN_THUMB, (track * m.viewport) / total));
  const offset = ((track - size) * clampTop(m, top)) / maxTop(m);
  return { size, offset };
}

/** Virtual top for a thumb at `offset` DOM pixels. */
export function topForThumb(m: ScrollModel, track: number, offset: number): number {
  const { size } = thumb(m, track, 0);
  const room = track - size;
  if (room <= 0) return 0;
  return clampTop(m, (offset / room) * maxTop(m));
}
