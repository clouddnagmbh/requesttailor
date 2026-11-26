// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { aiCapabilities, aiProviderKinds } from "../commands/contract.generated";
import { AI_PROVIDERS } from "@tailor-kit/ai-ui";

describe("AI providers (wave 16 interface)", () => {
  it("lists the same providers and capabilities as rt_app::ai, in the same order", () => {
    expect(AI_PROVIDERS.map((p) => p.kind)).toEqual(aiProviderKinds);
    expect(AI_PROVIDERS.map((p) => p.capabilities)).toEqual(aiCapabilities);
  });
});
