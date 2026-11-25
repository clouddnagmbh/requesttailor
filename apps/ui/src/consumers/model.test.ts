// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { ConsumerResult } from "../commands/types";
import { details, headline, isBroken, sourceLine, summary, tone } from "./model";

const ok: ConsumerResult = { name: "portal", requests: 1, failures: [] };
const broken: ConsumerResult = {
  name: "billing",
  requests: 1,
  skipped: 1,
  commit: "0123456789abcdef",
  failures: [{ request_path: "a.yml", request: "GET /orders/{id}", assertion: "x isNumber", difference: "got string" }],
};
const lost: ConsumerResult = { name: "ghost", requests: 0, failures: [], error: "cannot open" };

describe("consumers model", () => {
  it("tells ok, broken and could-not-run apart", () => {
    expect(tone(ok)).toBe("ok");
    expect(tone(broken)).toBe("danger");
    expect(tone(lost)).toBe("warn");
    expect([ok, broken, lost].map(isBroken)).toEqual([false, true, true]);
    expect(headline(ok)).toBe("portal is fine");
    expect(headline(broken)).toBe("billing breaks (1 failure)");
    expect(headline(lost)).toBe("ghost could not be verified");
  });

  it("writes details and a summary", () => {
    expect(details(broken)).toBe("1 request, 1 left out, commit 0123456");
    expect(details(ok)).toBe("1 request");
    expect(summary({ consumers: [ok, broken, lost] })).toBe("2 of 3 consumers broken");
    expect(sourceLine("../billing")).toBe("../billing");
    expect(sourceLine("https://h/x.git", "v2")).toBe("https://h/x.git @ v2");
  });
});
