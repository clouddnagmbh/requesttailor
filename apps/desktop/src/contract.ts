// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ContractApi` of the desktop app (wave 12, strand F; src-tauri/src/contract.rs): one Tauri
// command per method.

import type { ContractApi } from "@requesttailor/ui";

import type { Call, Command } from "./backend";

export function tauriContract(call: Call): ContractApi {
  return {
    spec: () => call("contract_spec"),
    check: (rev) => call("contract_check", { rev }),
    changes: (from, to) => call("contract_changes", { from, to }),
    changelog: (from, to) => call("contract_changelog", { from, to }),
  };
}

/** Commands of this strand no method calls yet; backend.test.ts leaves them out of its
 *  "every command is mapped" check. */
export const PENDING: readonly Command[] = [];
