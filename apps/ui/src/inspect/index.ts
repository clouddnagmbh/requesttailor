// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand B: this part's contributions to the response area (`response/slots.ts`): the
// chip "n checks" in the status line and the tab "Checks". Importing this file also registers the
// token warning with `shell/send-gates.ts` (once).

import type { ResponseSlotContribution } from "../response/slots";
import ChecksChip from "./ChecksChip.svelte";
import ChecksTab from "./ChecksTab.svelte";
import { registerTokenGate } from "./gate";

registerTokenGate();

const hasResponse: ResponseSlotContribution["show"] = (r) => r.response_id != null && r.report.response != null;

export const inspectSlots: ResponseSlotContribution[] = [
  { kind: "chip", id: "inspect-checks", component: ChecksChip, show: hasResponse },
  { kind: "tab", id: "inspect-checks", title: "Checks", component: ChecksTab, show: hasResponse },
];
