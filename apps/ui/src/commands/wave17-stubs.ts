// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 17 interface: placeholder APIs that reject with `unsupported` until a strand replaces
// them in its own file (mock-*.ts here, the shells' own files in apps/web and apps/desktop).
// Exported from the package so the shells can use them until then. The placeholders of a group
// are removed by the integrator, not by the strand (the strand only stops using them).

import type { ContractApi, ShareApi, SnapshotApi } from "./backend";
import { BackendError } from "./backend";

function notYet(feature: string, strand: string): () => Promise<never> {
  return () => Promise.reject(new BackendError("unsupported", `${feature} is not implemented yet (wave 17, strand ${strand}).`));
}

export function stubSnapshots(): SnapshotApi {
  const no = notYet("Snapshots", "B");
  return { read: no, freeze: no, setIgnore: no, check: no, list: no };
}

export function stubShare(): ShareApi {
  const no = notYet("Sharing", "C");
  return { link: no, open: no, copyAs: no, qr: no };
}

/** The placeholder for `ContractApi.stageSpec` (strand A). */
export function stubStageSpec(): ContractApi["stageSpec"] {
  return notYet("Staging the spec", "A");
}
