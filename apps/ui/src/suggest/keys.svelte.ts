// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// While a suggestion is on screen, Mod+Enter applies it instead of sending the request. The
// shell's global shortcut handler asks here before it runs "send".

let open = 0;

/** A suggestion view mounted (returns the function that unmounts it). */
export function suggestionOpened(): () => void {
  open++;
  return () => {
    open = Math.max(0, open - 1);
  };
}

export const suggestionOwnsEnter = (): boolean => open > 0;
