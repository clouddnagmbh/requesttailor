// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 12 interface: what the strands' mock parts (`mock-{diffs,merges,compare,timeline,
// review,contract,rescue}.ts`) get from the mock backend, so mock.ts stays untouched. The
// backend is still being constructed when a part is created: call these only inside methods.

import type { SendInput } from "./backend";
import type { Environment, RequestSpec, StoredResponse } from "./types";
import type { MockBackend } from "./mock";
import type { MockGit } from "./mock-git";

export interface MockWave12Host {
  /** The whole mock (its public `Backend` methods). */
  readonly backend: MockBackend;
  /** The mock's Git (commits, `treeAt`, …). */
  git(): MockGit;
  /** Requests in memory (the editor state). */
  requests(): Map<string, RequestSpec>;
  /** Requests as saved "on disk". */
  saved(): Map<string, RequestSpec>;
  environments(): Environment[];
  /** Sends `path` as the mock does (`scriptOutcome` included). */
  send(path: string, input: SendInput): Promise<StoredResponse>;
}
