<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Segments a source reported ("as reported by <source>"): a tree with offsets on the source's
     own clock. Never part of the sum of the client's phases above. -->
<script lang="ts">
  import { formatDuration } from "../response/format";
  import { layoutSegments, overWaitHint } from "../response/segments";
  import type { TimingSegment } from "../commands/types";

  let { source, segments, waitMs }: { source: string; segments: TimingSegment[]; waitMs: number } = $props();

  const layout = $derived(layoutSegments(segments));
  const hint = $derived(overWaitHint(layout, waitMs));
  const pct = (ms: number) => (layout.span > 0 ? (ms / layout.span) * 100 : 0);
</script>

<section class="segments" data-testid="segments-{source}" aria-label="Segments as reported by {source}">
  <h4>as reported by {source}</h4>
  <table>
    <tbody>
      {#each layout.rows as r (r.segment.id)}
        <tr data-segment={r.segment.id} data-depth={r.depth}>
          <th scope="row" style:padding-left="calc({r.depth} * var(--rt-space-4) + var(--rt-space-2))">{r.segment.label}</th>
          <td class="axis">
            <span
              class="bar"
              style:left="{pct(r.start)}%"
              style:width="max(2px, {pct(r.segment.ms)}%)"
              title="{r.segment.label}: {formatDuration(r.segment.ms)} (from {formatDuration(r.start)})"
            ></span>
          </td>
          <td class="value">{formatDuration(r.segment.ms)}</td>
        </tr>
      {/each}
    </tbody>
  </table>
  <p class="note">Not part of the total above: these numbers come from the {source}'s clock.</p>
  {#if hint}<p class="hint" data-testid="segments-hint-{source}">{hint}</p>{/if}
</section>

<style>
  .segments {
    padding: var(--rt-space-4) var(--rt-space-5);
    border-top: 1px solid var(--rt-border);
  }
  h4 {
    margin: 0 0 var(--rt-space-3);
    font: var(--rt-font-caption, inherit);
    color: var(--rt-fg-muted);
  }
  table {
    width: 100%;
    border-collapse: collapse;
  }
  th {
    width: 160px;
    padding: var(--rt-space-1) var(--rt-space-4) var(--rt-space-1) 0;
    text-align: left;
    font-weight: normal;
    white-space: nowrap;
  }
  .axis {
    position: relative;
    height: 16px;
  }
  .bar {
    position: absolute;
    top: 4px;
    height: 8px;
    border-radius: var(--rt-radius-sm);
    background: var(--rt-accent);
    opacity: 0.8;
  }
  .value {
    width: 70px;
    text-align: right;
    font-family: var(--rt-font-mono);
    font-size: var(--rt-size-small);
  }
  .note,
  .hint {
    margin: var(--rt-space-3) 0 0;
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
  .hint {
    color: var(--rt-warn);
  }
</style>
