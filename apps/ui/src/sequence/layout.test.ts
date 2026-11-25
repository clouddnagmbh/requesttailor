// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { SequenceModel } from "../commands/types";
import { bigModel } from "../commands/mock-sequence";
import { COLUMN_WIDTH, fitLabel, layoutSequence, MARGIN, NOTE_HEIGHT, ROW_HEIGHT, visibleGroups, visibleItems } from "./layout";

const small: SequenceModel = {
  participants: [
    { id: "client", label: "Client", kind: "client" },
    { id: "a", label: "a.example.com", kind: "server" },
    { id: "b", label: "b.example.com", kind: "server" },
  ],
  messages: [
    { kind: "request", from: "client", to: "a", label: "GET /x", request_path: "x.yml" },
    { kind: "response", from: "a", to: "client", label: "200 (1 ms)" },
    { kind: "request", from: "client", to: "b", label: "GET /y" },
    { kind: "response", from: "b", to: "client", label: "500 (2 ms)" },
  ],
  notes: [
    { after: 1, anchor: { kind: "over", participant: "a" }, text: "page 1/2" },
    { after: 3, anchor: { kind: "between", from: "client", to: "b" }, text: "1 of 3 tests failed" },
  ],
  groups: [{ kind: "loop", label: "2 pages", from: 0, to: 1 }],
  omitted: 0,
};

describe("layoutSequence", () => {
  it("puts the participants in columns and the messages in rows", () => {
    const l = layoutSequence(small);
    expect(l.columns.map((c) => c.x)).toEqual([MARGIN + 100, MARGIN + 300, MARGIN + 500]);
    expect(l.width).toBe(MARGIN * 2 + COLUMN_WIDTH * 3);
    expect(l.messageY).toEqual([0, ROW_HEIGHT, 2 * ROW_HEIGHT + NOTE_HEIGHT, 3 * ROW_HEIGHT + NOTE_HEIGHT]);
    const first = l.items[0];
    expect(first).toMatchObject({ type: "message", x1: MARGIN + 100, x2: MARGIN + 300, self: false, requestPath: "x.yml" });
  });

  it("is deterministic", () => {
    expect(JSON.stringify(layoutSequence(small))).toBe(JSON.stringify(layoutSequence(structuredClone(small))));
  });

  it("keeps notes inside the diagram and groups around their rows", () => {
    const l = layoutSequence(small);
    for (const item of l.items) {
      if (item.type === "note") {
        expect(item.x).toBeGreaterThanOrEqual(4);
        expect(item.x + item.width).toBeLessThanOrEqual(l.width);
      }
    }
    const g = l.groups[0];
    expect(g).toMatchObject({ kind: "loop", depth: 0, y1: 0, y2: 2 * ROW_HEIGHT + NOTE_HEIGHT });
  });

  it("nests groups and ignores impossible ones", () => {
    const l = layoutSequence({
      ...small,
      groups: [
        { kind: "rect", label: "outer", from: 0, to: 3 },
        { kind: "loop", label: "inner", from: 1, to: 1 },
        { kind: "alt", label: "bad", from: 3, to: 9 },
      ],
    });
    expect(l.groups.map((g) => [g.label, g.depth])).toEqual([["outer", 0], ["inner", 1]]);
  });

  it("draws a message to itself as a loop and survives unknown participants", () => {
    const l = layoutSequence({
      participants: [{ id: "client", label: "Client", kind: "client" }],
      messages: [{ kind: "request", from: "client", to: "client", label: "x.yml" }, { kind: "response", from: "ghost", to: "client", label: "?" }],
    });
    expect(l.items[0]).toMatchObject({ self: true });
    expect(l.items[1]).toMatchObject({ x1: l.columns[0]?.x });
  });

  it("lays out an empty model", () => {
    const l = layoutSequence({ participants: [], messages: [] });
    expect(l.items).toEqual([]);
    expect(l.width).toBeGreaterThan(0);
  });
});

describe("visibleItems", () => {
  const big = layoutSequence(bigModel());

  it("returns only the rows in the window", () => {
    const rows = visibleItems(big, 10_000, 600, 0);
    expect(rows.length).toBeGreaterThan(10);
    expect(rows.length).toBeLessThan(60);
    for (const r of rows) {
      expect(r.y + r.height).toBeGreaterThanOrEqual(10_000);
      expect(r.y).toBeLessThanOrEqual(10_600);
    }
  });

  it("finds nothing past the end and everything for a tall window", () => {
    expect(visibleItems(big, big.height + 5_000, 600, 0)).toEqual([]);
    expect(visibleItems(big, 0, big.height, 0).length).toBe(big.items.length);
    expect(visibleGroups(layoutSequence(small), 0, 100)).toHaveLength(1);
  });
});

describe("budget: sequence_render", () => {
  it("lays out 500 messages in well under 100 ms", () => {
    const model = bigModel();
    const five = { ...model, messages: model.messages.slice(0, 500), notes: (model.notes ?? []).filter((n) => n.after < 500) };
    layoutSequence(five);
    const times: number[] = [];
    for (let i = 0; i < 7; i++) {
      const t = performance.now();
      layoutSequence(five);
      times.push(performance.now() - t);
    }
    times.sort((a, b) => a - b);
    expect(times[3] ?? 0).toBeLessThan(100);
  });
});

describe("fitLabel", () => {
  it("cuts with an ellipsis", () => {
    expect(fitLabel("GET /orders", 400)).toBe("GET /orders");
    const cut = fitLabel("GET /a/very/long/path/that/does/not/fit", 100);
    expect(cut.endsWith("…")).toBe(true);
    expect(cut.length).toBeLessThan(20);
  });
});
