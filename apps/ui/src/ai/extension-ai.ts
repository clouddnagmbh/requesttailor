// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 16 interface: `ExtensionContext.ai`, the AI provider for extensions, over `Backend.ai`.
// The flow (policy, preview, confirmation, call) lives in `@tailor-kit/ai-ui`
// (`createAiClient`); this file supplies RequestTailor's part: the active environment as the
// input of a send, `BackendError`, and the shell's dialog place.

import { createAiClient, dialogConfirm as kitDialogConfirm, type ConfirmPreview } from "@tailor-kit/ai-ui";

import type { Backend, SendInput } from "../commands/backend";
import { BackendError } from "../commands/backend";
import type { AiScope } from "../commands/types";
import type { ExtensionAi } from "../extensions";
import type { AppState } from "../state/app.svelte";

export { nextCallId, type ConfirmPreview } from "@tailor-kit/ai-ui";

/** The preview in the shell's dialog place. */
export function dialogConfirm(app: AppState): ConfirmPreview {
  return kitDialogConfirm((dialog) => app.showDialog(dialog), app.backend.ai);
}

export function createExtensionAi(
  backend: Backend,
  app: AppState,
  confirm: ConfirmPreview = dialogConfirm(app),
): ExtensionAi {
  return createAiClient<AiScope, SendInput>({
    api: backend.ai,
    input: () => ({ environment: app.environment, vars: [] }),
    confirm,
    emptyScope: {},
    fail: (kind, message) => new BackendError(kind, message),
    optedOutMessage: "the AI assistant is off for this collection",
  });
}
