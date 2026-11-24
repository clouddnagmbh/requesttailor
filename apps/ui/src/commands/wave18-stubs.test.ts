// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { BackendError } from "./backend";
import * as stubs from "./wave18-stubs";

const GROUPS = {
  envCompare: stubs.stubEnvCompare(),
  inspect: stubs.stubInspect(),
  paging: stubs.stubPaging(),
  inbox: stubs.stubInbox(),
  lint: stubs.stubLint(),
  views: stubs.stubViews(),
  catalogue: stubs.stubCatalogue(),
};

describe("wave 18 placeholders", () => {
  it("reject every method with kind unsupported and name the wave", async () => {
    for (const [name, api] of Object.entries(GROUPS)) {
      for (const [method, fn] of Object.entries(api as unknown as Record<string, (...a: unknown[]) => Promise<unknown>>)) {
        const error = await fn().then(() => null, (e: unknown) => e);
        expect(error, `${name}.${method}`).toBeInstanceOf(BackendError);
        expect((error as BackendError).kind).toBe("unsupported");
        expect((error as BackendError).message).toContain("wave 18");
      }
    }
  });

  it("has a placeholder for the schema changes", async () => {
    await expect(stubs.stubSchemaChanges()("graphql", "a", "b")).rejects.toMatchObject({ kind: "unsupported" });
  });
});
