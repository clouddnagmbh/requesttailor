// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 12 interface: what the web shell's Git-for-APIs files (./diffs.ts, ./merges.ts,
// ./compare.ts, ./timeline.ts, ./review.ts, ./contract.ts, ./rescue.ts) get from the backend.
// Each file uses its own part of rt-wasm (`crates/rt-wasm/src/<strand>_js.rs`) and declares
// the types of those functions itself, as ./runner.ts does.

import type { GitApi, SendInput } from "@requesttailor/ui";

import type { RunContext } from "./runner";
import type { WebHost } from "./web-backend";

export interface Wave12Host {
  /** The open collection, the core and the companion. */
  readonly host: WebHost;
  /** The web Git, with the revisions of wave 12 (`treeAt`, `show`, …). */
  readonly git: GitApi;
  /** What a send needs (secret values, companion, services), as for the runner. */
  sendContext(input: SendInput): Promise<RunContext>;
}
