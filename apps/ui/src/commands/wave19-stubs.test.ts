// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { BackendError } from "./backend";
import * as stubs from "./wave19-stubs";

const GROUPS = {
  egress: stubs.stubEgress(),
  journal: stubs.stubJournal(),
  access: stubs.stubAccess(),
  shrink: stubs.stubShrink(),
  consumers: stubs.stubConsumers(),
  sequence: stubs.stubSequence(),
  security: stubs.stubSecurity(),
  notebook: stubs.stubNotebook(),
  debugger: stubs.stubDebugger(),
};

describe("wave 19 placeholders", () => {
  it("reject every method with kind unsupported and name the wave", async () => {
    for (const [name, api] of Object.entries(GROUPS)) {
      for (const [method, fn] of Object.entries(api as unknown as Record<string, (...a: unknown[]) => Promise<unknown>>)) {
        const error = await fn().then(() => null, (e: unknown) => e);
        expect(error, `${name}.${method}`).toBeInstanceOf(BackendError);
        expect((error as BackendError).kind).toBe("unsupported");
        expect((error as BackendError).message).toContain("wave 19");
      }
    }
  });
});
