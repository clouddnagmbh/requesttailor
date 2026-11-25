// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand C: palette command of paging (`BuiltinCommandSource`): the rule of the open
// request. Offered when a request is open.

import type { BuiltinCommandSource } from "../shell/builtin-commands";
import { openPagingRule } from "./open";

export const pagingCommands: BuiltinCommandSource = (app) => {
  const path = app.active;
  if (!app.collection || !path) return [];
  return [
    {
      id: "paging:rule",
      label: "Paging rule…",
      run: async () => {
        const responseId = app.results[path]?.response_id ?? null;
        const current = await app.backend.paging.rule(path, responseId).catch(() => null);
        await openPagingRule(app, path, responseId, current);
      },
    },
  ];
};
