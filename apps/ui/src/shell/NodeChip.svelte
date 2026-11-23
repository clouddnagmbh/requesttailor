<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The small chip an extension's node action (`inline: true`) shows on a collection or folder row
     (wave 14, strand C; the sidebar, strand A, places it). A button of its own, so a click does
     not select or fold the row. -->
<script lang="ts">
  import type { ExtensionNodeAction } from "../extensions";

  let { action }: { action: ExtensionNodeAction } = $props();

  function run(e: Event) {
    e.stopPropagation();
    try {
      void Promise.resolve(action.run()).catch((err) => console.warn("RequestTailor: node action failed:", err));
    } catch (err) {
      console.warn("RequestTailor: node action failed:", err);
    }
  }
</script>

<button
  type="button"
  class="chip"
  data-testid="node-chip"
  data-icon={action.icon}
  title={action.label}
  onclick={run}
  onkeydown={(e) => e.stopPropagation()}
>{action.label}</button>

<style>
  .chip {
    flex: none;
    max-width: 8em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    padding: 0 6px;
    height: 16px;
    line-height: 14px;
    font: inherit;
    font-size: 10px;
    color: var(--rt-accent);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: 8px;
    cursor: pointer;
  }
  .chip:hover {
    background: var(--rt-bg-hover);
    border-color: var(--rt-accent);
  }
  .chip:focus-visible {
    outline: 2px solid var(--rt-focus);
    outline-offset: 1px;
  }
</style>
