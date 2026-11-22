// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `PluginApi` of the desktop app (src-tauri/src/plugins.rs). Owner: wave 5, strand A.

import type { PluginApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriPlugins(call: Call): PluginApi {
  // Tauri passes a Uint8Array argument through as the raw request body.
  const raw = (data: Uint8Array) => data as unknown as Record<string, unknown>;
  return {
    list: () => call("plugin_list"),
    inspect: (file) => call("plugin_inspect", raw(file.data)),
    install: (file) => call("plugin_install", raw(file.data)),
    remove: (id) => call("plugin_remove", { id }),
    setEnabled: (id, enabled) => call("plugin_set_enabled", { id, enabled }),
    renderTab: (plugin, tab, target, state, environment) =>
      call("plugin_render_tab", { plugin, tab, target, state, environment: environment ?? null }),
    action: (plugin, tab, action, target, state, input, environment) =>
      call("plugin_action", { plugin, tab, action, target, state, input, environment: environment ?? null }),
    welcome: () => call("plugin_welcome"),
    chooseWelcome: (plugins) => call("plugin_choose_welcome", { plugins }),
  };
}
