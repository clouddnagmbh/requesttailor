<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The dialog of `ExtensionContext.suggest` (wave 16, strand C): the suggestion as a diff in
     the extension dialog place. Apply writes the unsaved change and offers an undo; closing the
     dialog by any other way discards. -->
<script lang="ts">
  import type { ExtensionDialogProps } from "../extensions";
  import { getApp } from "../state/app.svelte";
  import type { Prepared } from "./apply";
  import SuggestedEditView from "./SuggestedEditView.svelte";
  import { suggestUndoFor } from "./undo.svelte";

  let { close, props }: ExtensionDialogProps = $props();
  // svelte-ignore state_referenced_locally
  const prepared = props.prepared as Prepared;
  const app = getApp();
  let busy = $state(false);
  let error = $state<string | null>(null);

  async function apply() {
    if (busy) return;
    busy = true;
    error = null;
    try {
      const applied = await prepared.apply();
      suggestUndoFor(app).push({ title: prepared.title, subtitle: prepared.subtitle, undo: applied.undo });
      close(true);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      busy = false;
    }
  }
</script>

<SuggestedEditView
  blocks={prepared.blocks}
  subtitle={prepared.subtitle}
  note={prepared.note}
  {busy}
  {error}
  undoable={prepared.undoable}
  ondismiss={prepared.dismissKey ? () => close("dismissed") : undefined}
  onapply={apply}
  ondiscard={() => close(false)}
/>
