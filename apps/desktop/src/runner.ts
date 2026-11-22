// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `RunnerApi` of the desktop app (src-tauri/src/runner.rs). Progress arrives over a Tauri
// channel. Owner: wave 7, strand D.

import { Channel } from "@tauri-apps/api/core";
import type { RunEvent, RunnerApi } from "@requesttailor/ui";

import type { Call } from "./backend";

/** Creates the channel the events arrive on; injectable for tests (Tauri's needs a window). */
export type MakeChannel = (onEvent: (event: RunEvent) => void) => unknown;

const tauriChannel: MakeChannel = (onEvent) => {
  const channel = new Channel<RunEvent>();
  channel.onmessage = onEvent;
  return channel;
};

export function tauriRunner(call: Call, makeChannel: MakeChannel = tauriChannel): RunnerApi {
  return {
    plan: (folder) => call("runner_plan", { folder }),
    parseData: (file) => call("runner_parse_data", { name: file.name, data: [...file.data] }),
    run: (plan, input, onEvent) => {
      const channel = makeChannel(onEvent);
      // Secret values come from the keychain on the Rust side, never from the webview.
      return call("runner_run", {
        plan,
        input: { environment: input.environment, vars: input.vars },
        onEvent: channel,
      });
    },
    cancel: () => call("runner_cancel"),
  };
}
