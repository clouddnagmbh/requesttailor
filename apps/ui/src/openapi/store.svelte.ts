// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// What the OpenAPI host (`OpenApiHost.svelte`, mounted once by the shell) shows: the import
// wizard and the run wizard's "Ready to send?" panel. Entry points (Import dialog, "New…",
// palette, dropping a spec on the window) call `openOpenApiWizard`.

import type { ImportSource } from "../commands/backend";
import type { SendRequirements } from "../commands/types";
import type { GateController, GateResult } from "./gate";

export interface WizardRequest {
  seq: number;
  /** Files to start with (skips the source step), or `null`. */
  files: ImportSource[] | null;
}

export interface PanelRequest {
  seq: number;
  path: string;
  requirements: SendRequirements;
  controller: GateController;
  resolve: (result: GateResult) => void;
}

// Raw: the files' bytes and the gate's controller stay the objects they are (no proxies).
class OpenApiUi {
  wizard = $state.raw<WizardRequest | null>(null);
  panel = $state.raw<PanelRequest | null>(null);
}

export const openapiUi = new OpenApiUi();

let seq = 0;

/** Opens the OpenAPI import wizard, optionally with the spec's files already chosen. */
export function openOpenApiWizard(files: ImportSource[] | null = null): void {
  openapiUi.wizard = { seq: ++seq, files };
}

export function closeOpenApiWizard(): void {
  openapiUi.wizard = null;
}

/** Shows "Ready to send?" for `path`; resolves when the user decides. */
export function showReadyPanel(path: string, requirements: SendRequirements, controller: GateController): Promise<GateResult> {
  openapiUi.panel?.resolve("cancel");
  return new Promise((resolve) => {
    openapiUi.panel = {
      seq: ++seq,
      path,
      requirements,
      controller,
      resolve: (result) => {
        openapiUi.panel = null;
        resolve(result);
      },
    };
  });
}
