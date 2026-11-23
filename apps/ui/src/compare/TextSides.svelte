<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Two texts side by side with line numbers and the words that differ marked (wave 14,
     strand H, `worddiff.textRows`). Long texts show their first `limit` rows. -->
<script lang="ts">
  import Words from "./Words.svelte";
  import { textRows } from "./worddiff";

  let {
    old,
    now,
    hasOld = true,
    hasNew = true,
    limit = 2000,
  }: { old: string; now: string; hasOld?: boolean; hasNew?: boolean; limit?: number } = $props();

  const rows = $derived(textRows(hasOld ? old : "", hasNew ? now : ""));
  const shown = $derived(rows.slice(0, limit));
</script>

<div class="text" data-testid="cmp-text">
  {#each shown as r, i (i)}
    <div class="row {r.kind}" data-kind={r.kind}>
      <span class="no">{r.oldLine ?? ""}</span>
      <span class="cell old" class:empty={r.old === null}>{#if r.old}<Words segments={r.old} side="old" />{/if}</span>
      <span class="no">{r.newLine ?? ""}</span>
      <span class="cell new" class:empty={r.new === null}>{#if r.new}<Words segments={r.new} side="new" />{/if}</span>
    </div>
  {/each}
  {#if rows.length > shown.length}
    <p class="more">{rows.length - shown.length} more lines not shown.</p>
  {/if}
</div>

<style>
  .text {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto minmax(0, 1fr);
    font-family: var(--rt-font-mono);
    font-size: var(--rt-text-small);
    line-height: var(--rt-leading-body);
  }
  .row {
    display: contents;
  }
  .no {
    min-width: 3ch;
    padding: 0 var(--rt-space-3);
    background: var(--rt-surface-sunken);
    color: var(--rt-fg-subtle);
    text-align: right;
    user-select: none;
  }
  .cell {
    min-width: 0;
    padding: 0 var(--rt-space-4);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .row.changed .old,
  .row.removed .old {
    background: var(--rt-diff-del-bg);
  }
  .row.changed .new,
  .row.added .new {
    background: var(--rt-diff-add-bg);
  }
  .cell.empty {
    background: repeating-linear-gradient(
      -45deg,
      var(--rt-surface-sunken) 0 var(--rt-space-2),
      transparent var(--rt-space-2) var(--rt-space-4)
    );
  }
  .more {
    grid-column: 1 / -1;
    margin: var(--rt-space-2) var(--rt-space-4);
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
</style>
