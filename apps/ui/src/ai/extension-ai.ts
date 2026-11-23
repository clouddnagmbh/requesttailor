// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 16 interface: `ExtensionContext.ai`, the AI provider for extensions, over `Backend.ai`.
// The flow is fixed here; the backend does the masking and the call (strand A fills the shells),
// and strand A replaces the plain preview dialog (./PreviewDialog.svelte).
//
// 1. `policy()`: an opted-out collection refuses before anything is built.
// 2. `preview()`: the exact, masked call. If the settings require it (first use of a profile,
//    or always) or the caller asks, the user sees it and confirms; a first-use confirmation is
//    remembered (`confirmPreview`).
// 3. `complete()`: streamed pieces go to `onDelta`; aborting the signal cancels the call.

import type { Backend, SendInput } from "../commands/backend";
import { BackendError } from "../commands/backend";
import type { AiPreview, AiReply, AiRequest } from "../commands/types";
import type { ExtensionAi, ExtensionAiOptions, ExtensionAiRequest, ExtensionAiStatus } from "../extensions";
import type { AppState } from "../state/app.svelte";
import PreviewDialog from "./PreviewDialog.svelte";
import { providerInfo } from "./providers";

/** Asks the user to confirm a preview; `true` sends. */
export type ConfirmPreview = (preview: AiPreview) => Promise<boolean>;

let seq = 0;

/** A call id unique in this page. */
export function nextCallId(): string {
  seq += 1;
  return `ext-${Date.now().toString(36)}-${seq}`;
}

/** The preview in the shell's dialog place. */
export function dialogConfirm(app: AppState): ConfirmPreview {
  return async (preview) => {
    const answer = (await app.showDialog({
      title: "What will be sent",
      component: PreviewDialog,
      props: { preview },
      width: 720,
    })) as { send?: boolean; remember?: boolean } | undefined;
    if (answer?.send !== true) return false;
    if (answer.remember) {
      // "Don't show this automatically": previews only on request from now on.
      const settings = await app.backend.ai.settings();
      if (settings) await app.backend.ai.setSettings({ ...settings, preview: "on-request" });
    }
    return true;
  };
}

export function createExtensionAi(
  backend: Backend,
  app: AppState,
  confirm: ConfirmPreview = dialogConfirm(app),
): ExtensionAi {
  const input = (): SendInput => ({ environment: app.environment, vars: [] });
  return {
    async status(): Promise<ExtensionAiStatus> {
      const [settings, policy] = await Promise.all([backend.ai.settings(), backend.ai.policy()]);
      const profiles = (settings?.profiles ?? []).map((p) => {
        const caps = providerInfo(p.provider).capabilities;
        return { id: p.id, label: p.label, provider: p.provider, model: p.model, tools: caps.tools, streaming: caps.streaming };
      });
      const known = (id: string | undefined) => (id && profiles.some((p) => p.id === id) ? id : null);
      const fallback = known(settings?.default_profile) ?? profiles[0]?.id ?? null;
      return {
        configured: profiles.length > 0,
        allowed: policy.file_off === false && policy.device_off === false,
        profiles,
        fast: known(settings?.roles.fast) ?? fallback,
        smart: known(settings?.roles.smart) ?? fallback,
      };
    },

    async complete(request: ExtensionAiRequest, options: ExtensionAiOptions = {}): Promise<AiReply> {
      const policy = await backend.ai.policy();
      if (policy.file_off || policy.device_off) {
        throw new BackendError("opted-out", "the AI assistant is off for this collection");
      }
      const full: AiRequest = { ...request, id: nextCallId(), scope: request.scope ?? {} };
      const preview = await backend.ai.preview({ kind: "request", request: full }, input());
      if (preview.required || options.preview) {
        if (!(await confirm(preview))) throw new BackendError("cancelled", "cancelled");
        if (preview.required) await backend.ai.confirmPreview(preview.profile);
      }
      if (options.signal?.aborted) throw new BackendError("cancelled", "cancelled");
      const abort = () => void backend.ai.cancel(full.id);
      options.signal?.addEventListener("abort", abort, { once: true });
      try {
        return await backend.ai.complete(full, input(), (event) => {
          if (event.kind === "delta") options.onDelta?.(event.delta);
        });
      } finally {
        options.signal?.removeEventListener("abort", abort);
      }
    },
  };
}
