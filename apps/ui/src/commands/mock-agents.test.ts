// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { mockAgents } from "./mock-agents";
import type { RequestSpec } from "./types";

function spec(method: string, extra: Partial<RequestSpec> = {}): RequestSpec {
  return {
    name: "r",
    seq: null,
    method,
    url: "",
    headers: [],
    params: [],
    body: { type: "none" },
    auth: { type: "inherit" },
    vars: [],
    settings: {},
    scripts: [],
    assertions: [],
    actions: [],
    ...extra,
  } as RequestSpec;
}

describe("mock agents.check", () => {
  const requests: Record<string, RequestSpec> = {
    "get.yml": spec("GET"),
    "post.yml": spec("POST"),
    "query.yml": spec("POST", { body: { type: "graphql", query: "{ a }", variables: "" } }),
    "mutation.yml": spec("POST", { body: { type: "graphql", query: "mutation { a }", variables: "" } }),
    "grpc.yml": spec("GRPC", { protocol: { type: "grpc" } as never }),
    "ws.yml": spec("WS", { protocol: { type: "websocket", messages: [], timeout_ms: null, keep_alive_ms: null } }),
  };
  const agents = mockAgents({ scripted: false, getRequest: (p) => requests[p]! });

  it("follows effect and policy", async () => {
    await agents.setPolicy({
      access: "read",
      environments: [
        { environment: "Local", access: "write", production: false },
        { environment: "PRD", access: "write", production: true },
        { environment: "Vault", access: "none", production: false },
      ],
    });
    expect(await agents.check("get.yml", "PRD")).toEqual({ effect: "read", verdict: { decision: "allowed" } });
    expect(await agents.check("query.yml", null)).toEqual({ effect: "read", verdict: { decision: "allowed" } });
    expect(await agents.check("post.yml", "Local")).toEqual({ effect: "write", verdict: { decision: "allowed" } });
    expect((await agents.check("mutation.yml", "PRD")).verdict).toEqual({ decision: "needs-approval" });
    expect((await agents.check("grpc.yml", "Local")).effect).toBe("write");
    expect((await agents.check("ws.yml", "Local")).effect).toBe("read");
    expect((await agents.check("post.yml", null)).verdict.decision).toBe("refused");
    expect((await agents.check("get.yml", "Vault")).verdict.decision).toBe("refused");
  });
});
