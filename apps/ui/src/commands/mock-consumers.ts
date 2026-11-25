// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ConsumersApi` of the mock backend (wave 19, strand F): a small mirror of `rt_app::consumers`
// for UI tests (`?backend=mock`). The mock lists three consumers and plays the provider: a base
// URL that contains `break` makes `billing` break at `GET /orders/{orderId}` (the price came back
// as text), anything else keeps everyone green. The rules around it are the core's: a writing
// request of a consumer runs only with `allow_writes` (otherwise it is counted as left out), a
// name that is not listed is an error, and the report never holds a response.

import { BackendError, type ConsumersApi } from "./backend";
import type { ConsumerFailure, ConsumerRef, ConsumerResult } from "./types";

const CONSUMERS: ConsumerRef[] = [
  { name: "billing", source: "../billing", environment: "ci" },
  { name: "portal", source: "../portal", environment: "ci", vars: [["baseUrl", "{{provider.baseUrl}}"]] },
  { name: "mobile", source: "https://git.example.test/team/mobile.git", ref: "main", environment: "ci" },
];

const COMMIT = "0123456789abcdef0123456789abcdef01234567";

const BILLING_FAILURE: ConsumerFailure = {
  request_path: "get-order.yml",
  request: "GET /orders/{orderId}",
  assertion: "res.body.items[0].price isNumber",
  difference: "expected `res.body.items[0].price` isNumber, got string",
};

export function mockConsumers(): ConsumersApi {
  return {
    list: async () => CONSUMERS.map((c) => ({ ...c })),
    verify: async (request) => {
      const only = request.only ?? [];
      const unknown = only.find((n) => !CONSUMERS.some((c) => c.name === n));
      if (unknown) {
        throw new BackendError("format", `no consumer named "${unknown}" (listed: ${CONSUMERS.map((c) => c.name).join(", ")})`);
      }
      const breaks = request.base_url.includes("break");
      const consumers: ConsumerResult[] = [];
      for (const c of CONSUMERS.filter((c) => only.length === 0 || only.includes(c.name))) {
        const result: ConsumerResult = { name: c.name, requests: 1, failures: [] };
        if (c.name === "billing") {
          // One reading and one writing request; the write needs `allow_writes`.
          if (request.allow_writes) result.requests = 2;
          else result.skipped = 1;
          if (breaks) result.failures.push({ ...BILLING_FAILURE });
        }
        if (c.name === "mobile") result.commit = COMMIT;
        consumers.push(result);
        if (request.keep_going === false && result.failures.length > 0) break;
      }
      return { consumers };
    },
  };
}
