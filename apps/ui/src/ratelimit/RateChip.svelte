<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- What the response said about rate limits, in the status line ("47 / 60 left, resets in
     12 s"; "Rate limited, retry in 12 s" after a 429). Only there when it said something. -->
<script lang="ts">
  import type { ResponseSlotProps } from "../response/slots";
  import { rateChip } from "./model";

  let { response }: ResponseSlotProps = $props();

  const chip = $derived(response.report.rate_limit ? rateChip(response.report.rate_limit, response.report.response?.status) : null);
</script>

{#if chip}
  <span class="rate {chip.tone}" data-testid="ratelimit-chip" data-tone={chip.tone} title={chip.title}>{chip.text}</span>
{/if}

<style>
  .rate {
    padding: 0 var(--rt-space-2);
    border-radius: var(--rt-radius-pill);
    border: 1px solid var(--rt-info-border);
    background: var(--rt-info-bg);
    color: var(--rt-info-fg);
    font-size: var(--rt-size-small);
    white-space: nowrap;
  }
  .rate.warn {
    border-color: var(--rt-warn-border);
    background: var(--rt-warn-bg);
    color: var(--rt-warn-fg);
  }
  .rate.bad {
    border-color: var(--rt-danger-border);
    background: var(--rt-danger-bg);
    color: var(--rt-danger-fg);
  }
</style>
