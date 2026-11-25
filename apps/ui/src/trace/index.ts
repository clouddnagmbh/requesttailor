// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand A: this part's contributions to the response area (`response/slots.ts`):
// the "Trace id" chip in the status line. (Strand R adds the "Security" hints.)

import type { ResponseSlotContribution } from "../response/slots";
import TraceChip from "./TraceChip.svelte";

export const traceSlots: ResponseSlotContribution[] = [
  { kind: "chip", id: "trace-id", component: TraceChip, show: (r) => r.report.trace !== undefined },
];
