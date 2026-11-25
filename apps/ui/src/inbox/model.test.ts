// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { CapturedRequest, InboxDraft, InboxEvent } from "../commands/types";
import { planAdditions } from "../plugins/additions";
import { applyEvents, bodyView, cleanSettings, matches, proposal, ROW_HEIGHT, summary, trimTo, visibleRange } from "./model";

function req(id: number, over: Partial<CapturedRequest> = {}): CapturedRequest {
  return { id, at: 0, method: "POST", target: `/hooks/${id}`, headers: [], body: null, size: 0, ...over };
}
const event = (r: CapturedRequest): InboxEvent => ({ kind: "request", request: r });

describe("applyEvents", () => {
  it("appends requests once and in order", () => {
    const a = applyEvents([], [event(req(1)), event(req(2))]);
    expect(a.map((r) => r.id)).toEqual([1, 2]);
    expect(applyEvents(a, [event(req(2)), event(req(3))]).map((r) => r.id)).toEqual([1, 2, 3]);
  });

  it("empties on cleared and keeps the list on dropped, started and stopped (trimTo does the rest)", () => {
    const list = applyEvents([], [1, 2, 3, 4].map((i) => event(req(i))));
    expect(applyEvents(list, [{ kind: "dropped", count: 2 }])).toBe(list);
    expect(trimTo(list, 2).map((r) => r.id)).toEqual([3, 4]);
    expect(trimTo(list, 9)).toBe(list);
    expect(applyEvents(list, [{ kind: "cleared" }])).toEqual([]);
    expect(applyEvents(list, [{ kind: "started", port: 1 }, { kind: "stopped" }])).toBe(list);
  });

  it("never shows more than the buffer holds", () => {
    const many = Array.from({ length: 520 }, (_, i) => event(req(i + 1)));
    const list = applyEvents([], many);
    expect(list).toHaveLength(500);
    expect(list[0].id).toBe(21);
  });
});

describe("search", () => {
  const r = req(1, { headers: [["X-Event", "order.created"]], body: '{"customer":"Ada"}' });
  it("looks in the line, the headers and the body", () => {
    expect(matches(r, "")).toBe(true);
    expect(matches(r, "HOOKS/1")).toBe(true);
    expect(matches(r, "order.created")).toBe(true);
    expect(matches(r, "ada")).toBe(true);
    expect(matches(r, "missing")).toBe(false);
    expect(summary(r)).toBe("POST /hooks/1");
  });
});

describe("bodyView", () => {
  it("pretty prints JSON and keeps other text", () => {
    const json = bodyView(req(1, { headers: [["Content-Type", "application/json"]], body: '{"a":1}' }));
    expect(json?.lang).toBe("json");
    expect(json?.text).toContain("\n");
    expect(bodyView(req(1, { body: "plain words" }))?.lang).toBe("text");
    expect(bodyView(req(1, { body: null }))).toBeNull();
  });
});

describe("visibleRange", () => {
  it("renders the window and a little around it", () => {
    expect(visibleRange(0, ROW_HEIGHT * 10, 500)).toEqual([0, 16]);
    expect(visibleRange(ROW_HEIGHT * 100, ROW_HEIGHT * 10, 500)).toEqual([94, 116]);
    expect(visibleRange(ROW_HEIGHT * 495, ROW_HEIGHT * 10, 500)).toEqual([489, 500]);
    expect(visibleRange(0, 300, 0)).toEqual([0, 0]);
  });
});

describe("cleanSettings", () => {
  it("clamps to what the core accepts", () => {
    expect(cleanSettings({ port: 99999, status: 1, body: "", delay_ms: 9000, content_type: "  " })).toEqual({
      port: 65535,
      status: 200,
      body: "",
      delay_ms: 5000,
    });
    expect(cleanSettings({ port: 0, status: 202, body: "ok", delay_ms: 5, content_type: "text/plain" }).content_type).toBe("text/plain");
  });
});

describe("proposal", () => {
  const draft: InboxDraft = {
    name: "POST /hooks/orders",
    spec: {
      name: "POST /hooks/orders",
      seq: null,
      method: "POST",
      url: "{{baseUrl}}/hooks/orders",
      headers: [{ name: "Authorization", value: "{{authorization}}", enabled: true }],
      params: [],
      body: { type: "none" },
      auth: { type: "inherit" },
      vars: [],
      settings: {},
      scripts: [],
      assertions: [],
      actions: [],
    } as unknown as InboxDraft["spec"],
    secrets: ["authorization"],
  };

  it("adds the request and the secrets without a value to the selected environment", () => {
    const p = proposal(draft, "Local");
    expect(p.create_requests).toHaveLength(1);
    expect(p.add_environments).toEqual([{ name: "Local", variables: [{ name: "authorization", value: "", secret: true }] }]);
    const plan = planAdditions(p, null, [{ name: "Local", variables: [], client_certificates: [], external_secrets: null }]);
    expect(plan.requests[0].name).toBe("POST /hooks/orders");
    expect(plan.environments[0].variables).toEqual([{ name: "authorization", value: null, enabled: true, secret: true }]);
  });

  it("proposes no environment when none is selected or nothing is secret", () => {
    expect(proposal(draft, null).add_environments).toEqual([]);
    expect(proposal({ ...draft, secrets: [] }, "Local").add_environments).toEqual([]);
  });
});
