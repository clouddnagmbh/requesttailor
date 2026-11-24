// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand G: sequence diagrams, one Tauri command per method (`src-tauri/src/sequence.rs`,
// placeholders until strand G).

import type { SequenceApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriSequence(call: Call): SequenceApi {
  return {
    model: (source, options) => call("sequence_model", { source, options }),
    render: (model, options) => call("sequence_render", { model, options }),
    clear: () => call("sequence_clear"),
  };
}
