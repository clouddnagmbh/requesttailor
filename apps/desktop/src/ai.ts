// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `AiApi` of the desktop app (src-tauri/src/ai.rs). Settings (profiles, roles, preview mode)
// live in the data folder, keys in the OS keychain, one per profile; the webview never reads a
// key back. A call of an extension streams its events over a Tauri channel.

import { Channel } from "@tauri-apps/api/core";
import type { AiApi, AiEvent } from "@requesttailor/ui";

import type { Call } from "./backend";

/** Creates the channel the events of `complete` arrive on; injectable for tests (Tauri's needs
 *  a window). */
export type MakeAiChannel = (onEvent: (event: AiEvent) => void) => unknown;

const tauriChannel: MakeAiChannel = (onEvent) => {
  const channel = new Channel<AiEvent>();
  channel.onmessage = onEvent;
  return channel;
};

// Secret values are never sent from the webview; the Rust side reads the keychain.
const plain = (input: { environment?: string | null; vars?: [string, string][] }) => ({
  environment: input.environment,
  vars: input.vars,
});

export function tauriAi(call: Call, makeChannel: MakeAiChannel = tauriChannel): AiApi {
  return {
    config: () => call("ai_config"),
    setConfig: (config) => call("ai_set_config", { config }),
    hasKey: (provider) => call("ai_has_key", { provider }),
    setKey: (provider, key) => call("ai_set_key", { provider, key }),
    deleteKey: (provider) => call("ai_delete_key", { provider }),
    ask: (task, input) => call("ai_ask", { task, input: plain(input) }),
    settings: () => call("ai_settings"),
    setSettings: (settings) => call("ai_set_settings", { settings }),
    hasProfileKey: (profile) => call("ai_has_profile_key", { profile }),
    setProfileKey: (profile, key) => call("ai_set_profile_key", { profile, key }),
    deleteProfileKey: (profile) => call("ai_delete_profile_key", { profile }),
    policy: () => call("ai_policy"),
    setDeviceOptOut: (off) => call("ai_set_device_opt_out", { off }),
    preview: (what, input) => call("ai_preview", { what, input: plain(input) }),
    confirmPreview: (profile) => call("ai_confirm_preview", { profile }),
    complete: (request, input, onEvent) =>
      call("ai_complete", { request, input: plain(input), onEvent: makeChannel((e) => onEvent?.(e)) }),
    cancel: (id) => call("ai_cancel", { id }),
  };
}
