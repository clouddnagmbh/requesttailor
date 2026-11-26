// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { isIconName } from "@tailor-kit/ui";
import { describe, expect, it } from "vitest";

import { NEW_ICONS } from "./icons";

describe("New… icons", () => {
  it("are all in the shared icon set", () => {
    for (const name of NEW_ICONS) expect(isIconName(name)).toBe(true);
  });
});
