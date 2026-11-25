// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand C: this part's contributions to the response area (`response/slots.ts`): the
// "Next page" chip. It asks the backend whether a rule applies and shows nothing if none does.

import type { ResponseSlotContribution } from "../response/slots";
import PagingChip from "./PagingChip.svelte";

export const pagingSlots: ResponseSlotContribution[] = [
  { kind: "chip", id: "paging", component: PagingChip, show: (r) => r.response_id !== null && r.report.response !== null },
];
