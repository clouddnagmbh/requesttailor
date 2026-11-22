// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `CompareApi` of the desktop app (wave 12, strand C; src-tauri/src/compare.rs): runs at a
// revision, comparisons and bisect. Bisect progress arrives over a Tauri channel, as the
// runner's. Secret values come from the keychain on the Rust side, never from the webview.

import { Channel } from "@tauri-apps/api/core";
import type { BisectStep, CompareApi } from "@requesttailor/ui";

import type { Call } from "./backend";

/** Creates the channel the steps arrive on; injectable for tests (Tauri's needs a window). */
export type MakeStepChannel = (onStep: (step: BisectStep) => void) => unknown;

const tauriChannel: MakeStepChannel = (onStep) => {
  const channel = new Channel<BisectStep>();
  channel.onmessage = onStep;
  return channel;
};

export function tauriCompare(call: Call, makeChannel: MakeStepChannel = tauriChannel): CompareApi {
  const input = (i: { environment: string | null; vars: [string, string][] }) => ({
    environment: i.environment,
    vars: i.vars,
  });
  return {
    run: (request, i) => call("compare_run", { request, input: input(i) }),
    compare: (request, i) => call("compare_revisions", { request, input: input(i) }),
    bisect: (request, i, onStep) =>
      call("compare_bisect", { request, input: input(i), onStep: makeChannel(onStep) }),
    cancel: () => call("compare_cancel"),
  };
}
