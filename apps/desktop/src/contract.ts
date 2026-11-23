// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ContractApi` of the desktop app (wave 12, strand F; src-tauri/src/contract.rs): one Tauri
// command per method.

import { stubChangelogFacts, type ContractApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriContract(call: Call): ContractApi {
  return {
    spec: () => call("contract_spec"),
    check: (rev) => call("contract_check", { rev }),
    changes: (from, to) => call("contract_changes", { from, to }),
    changelog: (from, to) => call("contract_changelog", { from, to }),
    // Wave 16, strand D.
    changelogFacts: stubChangelogFacts(),
  };
}
