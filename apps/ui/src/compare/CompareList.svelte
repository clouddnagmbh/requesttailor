<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The requests of a comparison, one line each (wave 14, strand H): state, name, path and
     what differs. Virtualized (fixed row height), so a thousand requests render as fast as
     ten. ↑/↓, Home/End move the selection; the panel's n/p jump between changes. -->
<script lang="ts">
  import { Badge } from "../ui";
  import type { CompareRow } from "./rows";
  import { rowSummary } from "./rows";

  let {
    rows,
    selected,
    onselect,
  }: { rows: CompareRow[]; selected: number; onselect: (index: number) => void } = $props();

  const ROW = 44;
  const OVERSCAN = 6;
  let viewport = $state<HTMLDivElement | null>(null);
  let scrollTop = $state(0);
  let height = $state(400);

  const first = $derived(Math.max(0, Math.floor(scrollTop / ROW) - OVERSCAN));
  const last = $derived(Math.min(rows.length, Math.ceil((scrollTop + height) / ROW) + OVERSCAN));
  const visible = $derived(rows.slice(first, last));

  const STATE_LABEL = { new: "New", removed: "Removed", changed: "Changed", same: "Same" } as const;
  const STATUS = { new: "A", removed: "D", changed: "M" } as const;

  /** Keeps the selected row in view (after n/p or the arrow keys). */
  $effect(() => {
    const i = selected;
    if (!viewport || i < 0) return;
    const top = i * ROW;
    if (top < viewport.scrollTop) viewport.scrollTop = top;
    else if (top + ROW > viewport.scrollTop + viewport.clientHeight) viewport.scrollTop = top + ROW - viewport.clientHeight;
  });

  function onkeydown(e: KeyboardEvent) {
    const n = rows.length;
    if (n === 0) return;
    const page = Math.max(1, Math.floor(height / ROW) - 1);
    const to =
      e.key === "ArrowDown" ? Math.min(n - 1, selected + 1)
      : e.key === "ArrowUp" ? Math.max(0, selected - 1)
      : e.key === "Home" ? 0
      : e.key === "End" ? n - 1
      : e.key === "PageDown" ? Math.min(n - 1, selected + page)
      : e.key === "PageUp" ? Math.max(0, selected - page)
      : null;
    if (to === null) return;
    e.preventDefault();
    onselect(to);
  }
</script>

<div
  class="viewport"
  bind:this={viewport}
  bind:clientHeight={height}
  onscroll={() => (scrollTop = viewport?.scrollTop ?? 0)}
  role="listbox"
  tabindex="0"
  aria-label="Requests"
  aria-activedescendant={selected >= 0 ? `cmp-row-${selected}` : undefined}
  data-testid="compare-list"
  {onkeydown}
>
  <div class="spacer" style="height: {rows.length * ROW}px">
    {#each visible as r, k (r.path)}
      {@const i = first + k}
      <div
        id="cmp-row-{i}"
        class="row"
        role="option"
        tabindex="-1"
        aria-selected={i === selected}
        style="top: {i * ROW}px; height: {ROW}px"
        data-testid="compare-row"
        data-path={r.path}
        data-state={r.state}
        data-same={r.state === "same" && r.breaking.length === 0}
        onclick={() => onselect(i)}
        onkeydown={() => {}}
      >
        <span class="state" title={STATE_LABEL[r.state]}>
          {#if r.state === "same"}<span class="same" aria-label={STATE_LABEL[r.state]}></span>{:else}<Badge kind="status" value={STATUS[r.state]} />{/if}
        </span>
        <span class="text">
          <span class="line1">
            <span class="name">{r.name}</span>
            {#if r.breaking.length > 0}<Badge tone="danger" value="Breaking" />{/if}
            {#if r.regressed.length > 0}<span class="flag" data-testid="compare-row-regressed"><Badge tone="danger" value="green → red" /></span>{/if}
          </span>
          <span class="line2" data-testid="compare-row-summary">{rowSummary(r)}</span>
        </span>
      </div>
    {/each}
  </div>
</div>

<style>
  .viewport {
    position: relative;
    overflow: auto;
    min-height: 0;
    height: 100%;
    outline: none;
  }
  .viewport:focus-visible {
    box-shadow: inset 0 0 0 var(--rt-focus-width) var(--rt-focus);
  }
  .spacer {
    position: relative;
  }
  .row {
    position: absolute;
    left: var(--rt-space-3);
    right: var(--rt-space-3);
    display: flex;
    align-items: center;
    gap: var(--rt-space-4);
    padding: 0 var(--rt-space-4);
    box-sizing: border-box;
    border-radius: var(--rt-radius-md);
    cursor: pointer;
    transition: background var(--rt-dur-fast) var(--rt-ease);
  }
  .row:hover {
    background: var(--rt-bg-hover);
  }
  .row[aria-selected="true"] {
    background: var(--rt-accent-subtle);
    color: var(--rt-accent-subtle-fg);
  }
  .row[aria-selected="true"] .line2 {
    color: inherit;
    opacity: 0.85;
  }
  .state {
    display: inline-flex;
    justify-content: center;
    flex: none;
    width: var(--rt-control-sm);
  }
  .same {
    width: var(--rt-space-3);
    height: var(--rt-space-3);
    border-radius: var(--rt-radius-pill);
    background: var(--rt-border-strong);
  }
  .text {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-1);
    min-width: 0;
  }
  .line1 {
    display: flex;
    gap: var(--rt-space-3);
    align-items: center;
    min-width: 0;
    font: var(--rt-font-body);
  }
  .name {
    min-width: 0;
    font-weight: var(--rt-weight-medium);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .line2 {
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .flag {
    display: inline-flex;
    flex: none;
  }
  .line1 > :global(.rt-badge) {
    flex: none;
  }
</style>
