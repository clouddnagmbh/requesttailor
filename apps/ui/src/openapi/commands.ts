// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Palette entries of the OpenAPI wizard (the shell lists them after its own commands).

import { openOpenApiWizard } from "./store.svelte";

export function openApiCommands(): { id: string; label: string; run: () => void }[] {
  return [{ id: "import-openapi", label: "Import OpenAPI…", run: () => openOpenApiWizard() }];
}
