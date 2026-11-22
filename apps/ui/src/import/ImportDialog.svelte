<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Import dialog: pick or paste a file, see the loss report, write the new collection
     (`backend.imports`). Owner: wave 4, strand B. -->
<script lang="ts">
  import type { CollectionInfo } from "../commands/backend";

  let { open, onclose }: { open: boolean; onclose: () => void; onopen: (info: CollectionInfo) => Promise<void> } =
    $props();
</script>

{#if open}
  <div class="placeholder" role="dialog" aria-label="Import" data-testid="import-dialog">
    Import arrives in wave 4. <button onclick={onclose}>Close</button>
  </div>
{/if}

<style>
  .placeholder {
    position: fixed;
    inset: 30% 30% auto;
    padding: 16px;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg-panel);
  }
</style>
