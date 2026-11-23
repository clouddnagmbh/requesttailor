// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { MockBackend } from "./mock";
import { stubCreatePull } from "./wave15-stubs";

describe("wave 15 interface placeholders", () => {
  it("reject with kind unsupported until a strand lands", async () => {
    await expect(stubCreatePull({ title: "T", body: "", head: "a", base: "main", draft: false })).rejects.toMatchObject({ kind: "unsupported" });
    const b = new MockBackend();
    await expect(b.git.stage(["/a"])).rejects.toMatchObject({ kind: "unsupported" });
  });
});
