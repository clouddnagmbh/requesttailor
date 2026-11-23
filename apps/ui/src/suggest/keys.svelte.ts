// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// While a suggestion is on screen, Mod+Enter applies it instead of sending the request. The
// shell's global shortcut handler asks here before it runs "send".

let open = 0;
let inlineOpen = $state(0);

/** A suggestion view mounted (returns the function that unmounts it). An `inline` one is a
 *  proposal in an editor: the shell gives the editor pane more height while it is open. */
export function suggestionOpened(inline = false): () => void {
  open++;
  if (inline) inlineOpen++;
  return () => {
    open = Math.max(0, open - 1);
    if (inline) inlineOpen = Math.max(0, inlineOpen - 1);
  };
}

export const suggestionOwnsEnter = (): boolean => open > 0;

/** Whether a proposal is shown inside an editor (reactive). */
export const inlineProposalOpen = (): boolean => inlineOpen > 0;
