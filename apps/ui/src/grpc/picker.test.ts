// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { mockGrpc, MOCK_GRPC_SERVICES } from "../commands/mock-grpc";
import type { GrpcSpec } from "../commands/types";
import { chooseMethod, withTemplate } from "./picker";

const spec = (extra: Partial<GrpcSpec> = {}): GrpcSpec => ({
  method: "",
  method_type: "unary",
  proto_file: null,
  messages: [],
  ...extra,
});

describe("gRPC method picker", () => {
  it("sets the method type from the descriptors", () => {
    const chat = chooseMethod(spec(), MOCK_GRPC_SERVICES, "/rt.echo.v1.EchoService/Chat");
    expect(chat).toEqual(spec({ method: "/rt.echo.v1.EchoService/Chat", method_type: "bidi-streaming" }));
    const typed = chooseMethod(spec({ method_type: "server-streaming" }), MOCK_GRPC_SERVICES, "/x.Y/Z");
    expect(typed.method_type).toBe("server-streaming");
  });

  it("fills the selected message from the template", async () => {
    const template = await mockGrpc().template("a.yml", "/rt.echo.v1.EchoService/Countdown", {
      environment: null,
      vars: [],
    });
    expect(withTemplate(spec(), template).messages).toEqual([
      { title: "", selected: false, kind: "json", data: template },
    ]);
    const two = spec({
      messages: [
        { title: "a", selected: false, kind: "json", data: "{}" },
        { title: "b", selected: true, kind: "text", data: "x" },
      ],
    });
    expect(withTemplate(two, template).messages.map((m) => [m.title, m.kind, m.data])).toEqual([
      ["a", "json", "{}"],
      ["b", "json", template],
    ]);
  });

  it("rejects unknown methods in the mock", async () => {
    await expect(mockGrpc().template("a.yml", "/x.Y/Z", { environment: null, vars: [] })).rejects.toThrow(
      "unknown method",
    );
  });
});
