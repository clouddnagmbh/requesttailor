<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Open requests as tabs: click to switch, × or middle click to close, a dot for unsaved
     changes. Arrow keys move between tabs (ARIA tabs, automatic activation).
     Wave 14 (strand A): tabs of every collection in the sidebar (`AppState.openTabs`); opening
     one makes its collection active. Two tabs with the same name name their collection. -->
<script lang="ts">
  import { tick } from "svelte";

  import type { RequestRef } from "../commands/types";
  import { getApp } from "../state/app.svelte";
  import { Badge, IconButton } from "../ui";
  import { tabElementId } from "./tree";

  let { panelId }: { panelId: string } = $props();
  const app = getApp();
  let listEl = $state<HTMLElement>();

  const label = (ref: RequestRef) => app.draftOf(ref)?.name ?? ref.path.split("/").pop() ?? ref.path;
  const collectionName = (id: string) => app.collections.find((c) => c.id === id)?.name ?? "";
  const isSelected = (ref: RequestRef) => ref.collection === app.activeCollection && ref.path === app.active;
  /** Labels shared by tabs of different collections. */
  const ambiguous = $derived.by(() => {
    const seen = new Map<string, Set<string>>();
    for (const t of app.openTabs) {
      const l = label(t);
      seen.set(l, (seen.get(l) ?? new Set()).add(t.collection));
    }
    return new Set([...seen].filter(([, ids]) => ids.size > 1).map(([l]) => l));
  });

  async function selectAt(i: number) {
    const ref = app.openTabs[i];
    if (!ref) return;
    await app.selectRef(ref);
    await tick();
    listEl?.querySelector<HTMLElement>(`#${tabElementId(i)}`)?.focus();
  }

  function onkeydown(e: KeyboardEvent, i: number) {
    if (e.target !== e.currentTarget || e.metaKey || e.ctrlKey || e.altKey) return;
    const last = app.openTabs.length - 1;
    const to = { ArrowRight: i === last ? 0 : i + 1, ArrowLeft: i === 0 ? last : i - 1, Home: 0, End: last }[e.key];
    if (to === undefined) return;
    e.preventDefault();
    selectAt(to);
  }
</script>

{#if app.openTabs.length > 0}
  <div class="tabs" role="tablist" aria-label="Open requests" bind:this={listEl}>
    {#each app.openTabs as ref, i (`${ref.collection}\u0000${ref.path}`)}
      {@const selected = isSelected(ref)}
      {@const dirty = app.isDirty(ref)}
      {@const draft = app.draftOf(ref)}
      {@const name = label(ref)}
      {@const where = ambiguous.has(name) ? collectionName(ref.collection) : ""}
      <div
        class="tab"
        class:selected
        class:dirty
        role="tab"
        id={tabElementId(i)}
        data-testid="tab"
        data-path={ref.path}
        data-collection={ref.collection}
        aria-selected={selected}
        aria-controls={panelId}
        aria-label={`${name}${where ? ` (${where})` : ""}${dirty ? " (unsaved)" : ""}`}
        tabindex={selected ? 0 : -1}
        title="{collectionName(ref.collection)} › {ref.path}"
        onclick={() => app.selectRef(ref)}
        onauxclick={(e) => {
          if (e.button === 1) void app.closeRef(ref);
        }}
        onmousedown={(e) => {
          // No autoscroll cursor on middle click.
          if (e.button === 1) e.preventDefault();
        }}
        onkeydown={(e) => onkeydown(e, i)}
      >
        {#if draft}<span class="method"><Badge kind="method" value={draft.method} /></span>{/if}
        <span class="label">{name}</span>
        {#if where}<span class="where" data-testid="tab-collection">{where}</span>{/if}
        <span class="end">
          {#if dirty}<span class="dot" data-testid="tab-dirty" aria-hidden="true"></span>{/if}
          <span class="close" role="none" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
            <IconButton icon="close" size="sm" tabindex={-1} label="Close {name}" onclick={() => app.closeRef(ref)} />
          </span>
        </span>
      </div>
    {/each}
  </div>
{/if}

<style>
  .tabs {
    display: flex;
    overflow-x: auto;
    background: var(--rt-bg-panel);
    border-bottom: 1px solid var(--rt-border);
    scrollbar-width: thin;
  }
  .tab {
    position: relative;
    flex: none;
    display: flex;
    align-items: center;
    gap: var(--rt-space-3);
    max-width: 240px;
    height: var(--rt-bar-height);
    padding: 0 var(--rt-space-2) 0 var(--rt-space-5);
    border-right: 1px solid var(--rt-border);
    font: var(--rt-font-body);
    color: var(--rt-fg-muted);
    cursor: default;
    user-select: none;
    white-space: nowrap;
    transition: background-color var(--rt-dur-fast) var(--rt-ease);
  }
  .tab:hover {
    background: var(--rt-bg-hover);
  }
  .tab.selected {
    background: var(--rt-bg);
    color: var(--rt-fg);
  }
  .tab.selected::before {
    content: "";
    position: absolute;
    inset: 0 0 auto;
    height: 2px;
    background: var(--rt-accent-solid);
  }
  .tab:focus-visible {
    outline: 2px solid var(--rt-focus);
    outline-offset: -2px;
  }
  .method {
    flex: none;
    display: inline-flex;
  }
  .label {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .where {
    flex: none;
    max-width: 8em;
    overflow: hidden;
    text-overflow: ellipsis;
    font: var(--rt-font-small);
    color: var(--rt-fg-subtle);
  }
  /* The unsaved dot and the close button share one place; hovering shows the button. */
  .end {
    position: relative;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: var(--rt-control-sm);
    height: var(--rt-control-sm);
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: var(--rt-radius-pill);
    background: var(--rt-accent);
  }
  .close {
    position: absolute;
    inset: 0;
    display: inline-flex;
    opacity: 0;
  }
  .tab.selected:not(.dirty) .close,
  .tab:hover .close,
  .tab:focus-within .close {
    opacity: 1;
  }
  .tab:hover .dot,
  .tab:focus-within .dot {
    opacity: 0;
  }
</style>
