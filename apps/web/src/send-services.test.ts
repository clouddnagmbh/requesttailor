// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { SendCheck } from "@requesttailor/ui";

import { sendServices, type SendServiceParts } from "./send-services";

function parts(extra: Partial<SendServiceParts> = {}): SendServiceParts {
  const forSend = async () => undefined;
  return {
    plugins: { forSend } as unknown as SendServiceParts["plugins"],
    auth: { forSend } as unknown as SendServiceParts["auth"],
    cookies: { forSend } as unknown as SendServiceParts["cookies"],
    scriptTrust: { forSend: () => true },
    ...extra,
  };
}

describe("send services", () => {
  it("carry no guard in the builds of this repository", async () => {
    const services = await sendServices(parts(), "c", null);
    expect("guard" in services).toBe(false);
  });

  it("pass a distribution's guard to the core (wave 10)", async () => {
    const seen: SendCheck[] = [];
    const guard = (check: SendCheck) => {
      seen.push(check);
      return { decision: "refuse" as const, rule: "r", reason: "no" };
    };
    const services = await sendServices(parts({ sendGuard: guard }), "c", "PRD");
    expect(services.guard).toBe(guard);
  });
});
