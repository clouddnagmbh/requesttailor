// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 17: extensions get the production rule from the package instead of copying it.

import { describe, expect, it } from "vitest";

import { isProduction, isProductionName } from "../index";

describe("exported production rule", () => {
  it("is the one the app uses", () => {
    expect(isProduction(null, "prd")).toBe(true);
    expect(isProduction(null, null)).toBe(false);
    expect(isProduction({ environments: [{ environment: "prod-like", production: false }] } as never, "prod-like")).toBe(false);
    expect(isProductionName("live")).toBe(true);
  });
});
