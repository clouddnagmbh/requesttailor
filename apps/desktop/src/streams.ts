// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `StreamsApi` of the desktop app (src-tauri/src/streams.rs): sessions open natively, events
// arrive over a Tauri channel. Owner: wave 8, strand A.

import { Channel } from "@tauri-apps/api/core";
import type { StreamEvent, StreamsApi } from "@requesttailor/ui";

import type { Call } from "./backend";

/** Creates the channel the events arrive on; injectable for tests (Tauri's needs a window). */
export type MakeStreamChannel = (onEvent: (event: StreamEvent) => void) => unknown;

const tauriChannel: MakeStreamChannel = (onEvent) => {
  const channel = new Channel<StreamEvent>();
  channel.onmessage = onEvent;
  return channel;
};

export function tauriStreams(
  call: Call,
  makeChannel: MakeStreamChannel = tauriChannel,
): StreamsApi {
  return {
    open: (path, input, onEvent) =>
      // Secret values come from the keychain on the Rust side, never from the webview.
      call("stream_open", {
        path,
        input: { environment: input.environment, vars: input.vars },
        onEvent: makeChannel(onEvent),
      }),
    send: (session, message) => call("stream_send", { session, message }),
    finish: (session) => call("stream_finish", { session }),
    close: (session) => call("stream_close", { session }),
    log: (session) => call("stream_log", { session }),
    release: (session) => call("stream_release", { session }),
  };
}
