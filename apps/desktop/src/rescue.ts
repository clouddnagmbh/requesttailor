// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `RescueApi` of the desktop app (wave 12, strand G; src-tauri/src/rescue.rs): the history
// scan with its progress over a Tauri channel, and the rewrite. The value of a finding stays
// on the Rust side (it goes from the old blob into the keychain); the webview only sends the
// plan (finding ids, variable, environment) and gets masked findings and the result back.

import { Channel } from "@tauri-apps/api/core";
import type { RescueApi, ScanProgress } from "@requesttailor/ui";

import type { Call } from "./backend";

/** Creates the channel the progress arrives on; injectable for tests (Tauri's needs a window). */
export type MakeProgressChannel = (onProgress: (progress: ScanProgress) => void) => unknown;

const tauriChannel: MakeProgressChannel = (onProgress) => {
  const channel = new Channel<ScanProgress>();
  channel.onmessage = onProgress;
  return channel;
};

export function tauriRescue(call: Call, makeChannel: MakeProgressChannel = tauriChannel): RescueApi {
  return {
    scanHistory: (onProgress) => call("rescue_scan", { onProgress: makeChannel(onProgress ?? (() => {})) }),
    rewrite: (plan) =>
      call("rescue_rewrite", {
        plan: { findings: plan.findings, variable: plan.variable, environment: plan.environment },
      }),
  };
}
