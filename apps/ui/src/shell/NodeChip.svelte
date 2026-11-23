<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The small chip an extension's node action (`inline: true`) shows on a collection or folder row
     (wave 14, strand C; the sidebar, strand A, places it). A button of its own, so a click does
     not select or fold the row. -->
<script lang="ts">
  import type { ExtensionNodeAction } from "../extensions";
  import { Chip } from "../ui";

  let { action }: { action: ExtensionNodeAction } = $props();

  function run() {
    try {
      void Promise.resolve(action.run()).catch((err) => console.warn("RequestTailor: node action failed:", err));
    } catch (err) {
      console.warn("RequestTailor: node action failed:", err);
    }
  }
</script>

<!-- Stops the click and key events so the row neither selects nor folds. -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<span class="chip" role="presentation" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
  <Chip label={action.label} icon={action.icon} tone="accent" size="sm" onclick={run} title={action.label} data-testid="node-chip" data-icon={action.icon} />
</span>

<style>
  .chip {
    display: inline-flex;
    flex: none;
    max-width: 9em;
  }
</style>
