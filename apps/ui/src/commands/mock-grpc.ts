// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `GrpcApi` of the mock backend (wave 8, strand B): the echo service of `rt-testserver`
// (testdata/grpc/protos/echo.proto) for every gRPC request, templates as rt-grpc prints them.

import { BackendError, type GrpcApi } from "./backend";
import type { GrpcMethodType, GrpcService } from "./types";

const PKG = "rt.echo.v1";

const method = (name: string, method_type: GrpcMethodType, input: string, output: string) => ({
  name,
  path: `/${PKG}.EchoService/${name}`,
  method_type,
  input_type: `${PKG}.${input}`,
  output_type: `${PKG}.${output}`,
});

export const MOCK_GRPC_SERVICES: GrpcService[] = [
  {
    name: `${PKG}.EchoService`,
    methods: [
      method("Chat", "bidi-streaming", "EchoRequest", "EchoReply"),
      method("Collect", "client-streaming", "EchoRequest", "EchoReply"),
      method("Countdown", "server-streaming", "CountdownRequest", "EchoReply"),
      method("Echo", "unary", "EchoRequest", "EchoReply"),
      method("Fail", "unary", "FailRequest", "EchoReply"),
    ],
  },
];

const ECHO_REQUEST = `{
  "message": "",
  "priority": "PRIORITY_UNSPECIFIED",
  "tags": [
    ""
  ],
  "labels": {
    "": ""
  },
  "sentAt": "1970-01-01T00:00:00Z",
  "number": "0"
}`;

const TEMPLATES: Record<string, string> = {
  EchoRequest: ECHO_REQUEST,
  CountdownRequest: `{\n  "message": "",\n  "count": 0,\n  "delayMs": 0\n}`,
  FailRequest: `{\n  "code": 0,\n  "message": ""\n}`,
};

export function mockGrpc(): GrpcApi {
  const find = (path: string) => MOCK_GRPC_SERVICES.flatMap((s) => s.methods).find((m) => m.path === path);
  return {
    services: async () => structuredClone(MOCK_GRPC_SERVICES),
    template: async (_path, path) => {
      const m = find(path);
      if (!m) throw new BackendError("shell", `unknown method: ${path}`);
      return TEMPLATES[m.input_type.slice(PKG.length + 1)];
    },
  };
}
