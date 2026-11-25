// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand C: opens the "Paging rule" dialog (`PagingRuleDialog.svelte`) for a request.

import type { EffectivePaging } from "../commands/types";
import type { AppState } from "../state/app.svelte";
import PagingRuleDialog from "./PagingRuleDialog.svelte";

export function openPagingRule(
  app: AppState,
  requestPath: string,
  responseId: number | null,
  current: EffectivePaging | null,
): Promise<unknown> {
  return app.showDialog({
    title: "Paging rule",
    component: PagingRuleDialog,
    width: 560,
    props: { app, requestPath, responseId, current },
  });
}
