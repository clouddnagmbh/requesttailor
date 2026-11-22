// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// What the gRPC method picker does to a `GrpcSpec` (wave 8, strand B): pick a method (its
// type follows from the descriptors) and fill the selected message from a template.

import type { GrpcMethod, GrpcService, GrpcSpec } from "../commands/types";

export function findMethod(services: GrpcService[], path: string): GrpcMethod | undefined {
  return services.flatMap((s) => s.methods).find((m) => m.path === path);
}

/** `spec` with `path` as its method and the method's type; unknown paths keep the type. */
export function chooseMethod(spec: GrpcSpec, services: GrpcService[], path: string): GrpcSpec {
  const m = findMethod(services, path);
  return { ...spec, method: path, method_type: m?.method_type ?? spec.method_type };
}

/** `spec` with `template` as the data of its selected message (the first if none is
 *  selected; a new JSON message if there is none). */
export function withTemplate(spec: GrpcSpec, template: string): GrpcSpec {
  if (spec.messages.length === 0) {
    return { ...spec, messages: [{ title: "", selected: false, kind: "json", data: template }] };
  }
  const selected = Math.max(
    0,
    spec.messages.findIndex((m) => m.selected),
  );
  return {
    ...spec,
    messages: spec.messages.map((m, i) => (i === selected ? { ...m, kind: "json", data: template } : m)),
  };
}

/** Method types as the picker labels them. */
export const METHOD_TYPE_LABEL: Record<GrpcMethod["method_type"], string> = {
  unary: "Unary",
  "client-streaming": "Client streaming",
  "server-streaming": "Server streaming",
  "bidi-streaming": "Bidirectional streaming",
};
