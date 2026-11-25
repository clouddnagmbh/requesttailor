// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand W: the palette entry "Notebooks…".

import type { BuiltinCommandSource } from "../shell/builtin-commands";
import { openNotebook } from "./open.svelte";

export const notebookCommands: BuiltinCommandSource = (app) =>
  app.collection
    ? [{ id: "notebook:open", label: "Notebooks…", run: () => openNotebook(app) }]
    : [];
