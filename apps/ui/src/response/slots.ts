// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18 interface: what parts add to the response area besides the plugin and extension tabs:
// tabs ("Checks", "Views") and chips in the status line ("3 checks", "12 / 100 left"). Each part
// exports its list from `<part>/index.ts`; `ResponsePanel` reads `responseSlots()`.

import type { Component } from "svelte";

import type { StoredResponse } from "../commands/types";
import type { AppState } from "../state/app.svelte";
import { inspectSlots } from "../inspect";
import { pagingSlots } from "../paging";
import { ratelimitSlots } from "../ratelimit";
import { viewsSlots } from "../views";

export interface ResponseSlotProps {
  app: AppState;
  requestPath: string;
  response: StoredResponse;
}

export type ResponseSlotContribution =
  | { kind: "tab"; id: string; title: string; component: Component<ResponseSlotProps>; show?: (r: StoredResponse) => boolean }
  | { kind: "chip"; id: string; component: Component<ResponseSlotProps>; show?: (r: StoredResponse) => boolean };

export function responseSlots(): ResponseSlotContribution[] {
  return [...inspectSlots, ...viewsSlots, ...ratelimitSlots, ...pagingSlots];
}
