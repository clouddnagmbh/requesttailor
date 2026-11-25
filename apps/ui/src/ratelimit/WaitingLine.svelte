<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The runner's "Waiting 12 s (rate limit)" line, counting down while the core holds the next
     request back (`RunEvent.waiting`). -->
<script lang="ts">
  import { waitingText } from "./model";

  let { waiting }: { waiting: { request_path: string; until: number; reason: string } | null } = $props();

  let now = $state(Date.now());
  $effect(() => {
    if (!waiting) return;
    now = Date.now();
    const timer = setInterval(() => (now = Date.now()), 250);
    return () => clearInterval(timer);
  });
</script>

{#if waiting}
  <span class="waiting" data-testid="runner-waiting" title={waiting.reason} aria-live="polite">
    {waitingText(waiting.until, now)}: {waiting.request_path}
  </span>
{/if}

<style>
  .waiting {
    color: var(--rt-warn-fg);
  }
</style>
