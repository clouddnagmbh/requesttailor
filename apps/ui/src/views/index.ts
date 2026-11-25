// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand G: this part's contributions to the response area (`response/slots.ts`): the
// tab "Views" with the saved views of the request. The tab is offered for responses that read
// as JSON (the core decides what a view finds; this only keeps the tab off HTML and images).

import type { ResponseSlotContribution } from "../response/slots";
import { looksJson } from "./model";
import ViewsTab from "./ViewsTab.svelte";

export const viewsSlots: ResponseSlotContribution[] = [
  {
    kind: "tab",
    id: "views",
    title: "Views",
    component: ViewsTab,
    show: (r) => {
      const response = r.report.response;
      if (r.response_id == null || !response || !r.is_text) return false;
      const type = response.headers.find(([n]) => n.toLowerCase() === "content-type")?.[1] ?? null;
      const preview = response.body.encoding === "utf8" ? response.body.data.slice(0, 64) : "";
      return looksJson(type, preview);
    },
  },
];
