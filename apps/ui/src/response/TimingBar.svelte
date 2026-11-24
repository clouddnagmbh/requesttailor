<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The one-line timing bar (wave 17, strand D): the phases stacked in one bar and a sentence
     ("Server wait is 82 % of 410 ms"). The waterfall tab has the detail. -->
<script lang="ts">
  import type { TimingBarData } from "./timing-bar";

  let { data, sentence = true }: { data: TimingBarData; sentence?: boolean } = $props();
</script>

<span class="timing-bar" data-testid="timing-bar" title={data.title}>
  <span class="bar" role="img" aria-label={data.sentence}>
    {#each data.segments as s (s.name)}
      {#if s.pct > 0}<span class="seg {s.name}" data-phase={s.name} style:width="{s.pct}%"></span>{/if}
    {/each}
  </span>
  {#if sentence}<span class="sentence" data-testid="timing-bar-sentence">{data.sentence}</span>{/if}
</span>

<style>
  .timing-bar {
    display: inline-flex;
    align-items: center;
    gap: var(--rt-space-3);
    min-width: 0;
  }
  .bar {
    display: inline-flex;
    width: 120px;
    height: 8px;
    overflow: hidden;
    border-radius: var(--rt-radius-pill);
    background: var(--rt-surface-sunken, var(--rt-bg));
  }
  .seg {
    min-width: 2px;
    height: 100%;
  }
  .seg.dns {
    background: var(--rt-var-secret);
  }
  .seg.connect {
    background: var(--rt-warn);
  }
  .seg.tls {
    background: var(--rt-ok);
  }
  .seg.wait {
    background: var(--rt-accent);
  }
  .seg.transfer {
    background: var(--rt-fg-muted);
  }
  .sentence {
    color: var(--rt-fg-muted);
    white-space: nowrap;
  }
</style>
