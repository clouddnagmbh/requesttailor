// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ContractApi` of the desktop app (wave 12, strand F; src-tauri/src/contract.rs). Interface
// placeholder: the Tauri commands are registered and refuse; the strand calls them here.

import { stubContract, type ContractApi } from "@requesttailor/ui";

import type { Call, Command } from "./backend";

export function tauriContract(call: Call): ContractApi {
  void call;
  return stubContract();
}

/** Commands of this strand no method calls yet; backend.test.ts leaves them out of its
 *  "every command is mapped" check. The strand empties the list as it maps them. */
export const PENDING: readonly Command[] = [
  "contract_spec",
  "contract_check",
  "contract_changes",
  "contract_changelog",
];
