// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `SequenceApi` of the web app (wave 19, strand G), over rt-wasm's free functions
// (`crates/rt-wasm/src/sequence_js.rs`). The page's recorder sees every send the core makes, so a
// diagram is built from what really went out; the runner marks the start and end of a run with
// `markRun`. The web has no audit log and no core history: those sources reject.

import type { SequenceApi, SequenceModel, SequenceOptions, SequenceSource, SequenceText } from "@requesttailor/ui";

import { toBackendError, type CoreModule } from "./wasm";

/** The sequence part of the rt-wasm module. */
export interface CoreSequenceModule {
  sequenceModel(source: SequenceSource, options: SequenceOptions): SequenceModel;
  sequenceRender(model: SequenceModel, options: SequenceOptions): SequenceText;
  sequenceClear(): void;
  sequenceBeginRun(): number;
  sequenceEndRun(): void;
}

export interface SequenceHost {
  loadCore(): Promise<CoreModule>;
}

async function core(host: SequenceHost): Promise<CoreSequenceModule> {
  return (await host.loadCore()) as unknown as CoreSequenceModule;
}

/** Marks a run in the recorder: `begin` before the first request, `end` when it is over. */
export function markRun(module: CoreModule): { begin(): void; end(): void } {
  const m = module as unknown as Partial<CoreSequenceModule>;
  return {
    begin: () => void m.sequenceBeginRun?.(),
    end: () => void m.sequenceEndRun?.(),
  };
}

export function webSequence(host: SequenceHost): SequenceApi {
  const guard = async <T>(f: (m: CoreSequenceModule) => T): Promise<T> => {
    try {
      return f(await core(host));
    } catch (e) {
      throw toBackendError(e);
    }
  };
  return {
    model: (source, options) => guard((m) => m.sequenceModel(source, options)),
    render: (model, options) => guard((m) => m.sequenceRender(model, options)),
    clear: () => guard((m) => m.sequenceClear()),
  };
}
