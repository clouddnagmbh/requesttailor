// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand B: the settings of this device for the token warning (never in a collection
// file). A broken or missing storage leaves the warning on.

export const WARN_KEY = "rt.inspect.token-warning";

function storage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

/** Whether the warning is on (it is, unless this device switched it off). */
export function warningEnabled(): boolean {
  try {
    return storage()?.getItem(WARN_KEY) !== "off";
  } catch {
    return true;
  }
}

export function setWarningEnabled(on: boolean): void {
  try {
    if (on) storage()?.removeItem(WARN_KEY);
    else storage()?.setItem(WARN_KEY, "off");
  } catch {
    // No storage: the warning stays as it is.
  }
}
