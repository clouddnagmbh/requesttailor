<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Manage environments and secret values. Owner: wave 3, strand D. Placeholder. -->
<script lang="ts">
  let { open, onclose }: { open: boolean; onclose: () => void } = $props();
</script>

{#if open}
  <div role="dialog" aria-label="Environments" data-testid="env-manager">
    <button onclick={onclose}>Close</button>
  </div>
{/if}
