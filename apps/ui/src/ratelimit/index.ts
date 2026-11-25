// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand D: this part's contributions to the response area (`response/slots.ts`):
// the rate-limit chip in the status line.

import type { ResponseSlotContribution } from "../response/slots";
import RateChip from "./RateChip.svelte";

export const ratelimitSlots: ResponseSlotContribution[] = [
  { kind: "chip", id: "ratelimit", component: RateChip, show: (r) => r.report.rate_limit !== undefined },
];
