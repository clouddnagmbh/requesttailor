<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Open requests as tabs: click to switch, × or middle click to close, a dot for unsaved
     changes. Arrow keys move between tabs (ARIA tabs, automatic activation). The tabs share
     the width (tab-fit.ts); those that do not fit even at the minimum go behind a "N more"
     button that lists all tabs (TabList). Never a horizontal scrollbar. Context menu: close
     variants and "Reveal in sidebar".
     Wave 14 (strand A): tabs of every collection in the sidebar (`AppState.openTabs`); opening
     one makes its collection active. Two tabs with the same name name their collection. -->
<script lang="ts">
  import { tick } from "svelte";

  import type { RequestRef } from "../commands/types";
  import { getApp } from "../state/app.svelte";
  import { Badge, Icon, IconButton, Menu, type MenuEntry } from "../ui";
  import { fitTabs, TAB_MAX, TAB_MIN, TAB_MORE } from "./tab-fit";
  import TabList from "./TabList.svelte";
  import { tabElementId } from "./tree";

  let { panelId, onreveal }: { panelId: string; onreveal?: (ref: RequestRef) => unknown } = $props();
  const app = getApp();
  let listEl = $state<HTMLElement>();
  let moreEl = $state<HTMLElement>();
  let barWidth = $state(0);

  const label = (ref: RequestRef) => app.draftOf(ref)?.name ?? ref.path.split("/").pop() ?? ref.path;
  const collectionName = (id: string) => app.collections.find((c) => c.id === id)?.name ?? "";
  const isSelected = (ref: RequestRef) => ref.collection === app.activeCollection && ref.path === app.active;
  const same = (a: RequestRef, b: RequestRef) => a.collection === b.collection && a.path === b.path;
  /** Labels shared by tabs of different collections. */
  const ambiguous = $derived.by(() => {
    const seen = new Map<string, Set<string>>();
    for (const t of app.openTabs) {
      const l = label(t);
      seen.set(l, (seen.get(l) ?? new Set()).add(t.collection));
    }
    return new Set([...seen].filter(([, ids]) => ids.size > 1).map(([l]) => l));
  });

  const fit = $derived(fitTabs({ count: app.openTabs.length, active: app.activeTabIndex, available: barWidth }));

  async function selectAt(i: number) {
    const ref = app.openTabs[i];
    if (!ref) return;
    await app.selectRef(ref);
    await tick();
    listEl?.querySelector<HTMLElement>(`#${tabElementId(i)}`)?.focus();
  }

  function onkeydown(e: KeyboardEvent, i: number, ref: RequestRef) {
    if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) {
      e.preventDefault();
      openMenu(ref, e.currentTarget as HTMLElement);
      return;
    }
    if (e.target !== e.currentTarget || e.metaKey || e.ctrlKey || e.altKey) return;
    const last = app.openTabs.length - 1;
    const to = { ArrowRight: i === last ? 0 : i + 1, ArrowLeft: i === 0 ? last : i - 1, Home: 0, End: last }[e.key];
    if (to === undefined) return;
    e.preventDefault();
    selectAt(to);
  }

  // ---- context menu ----
  let menuOpen = $state(false);
  let menuFor = $state<RequestRef | null>(null);
  let menuAnchor = $state<HTMLElement | { x: number; y: number }>();

  function openMenu(ref: RequestRef, anchor: HTMLElement | { x: number; y: number }) {
    menuFor = ref;
    menuAnchor = anchor;
    menuOpen = true;
  }

  const menuItems = $derived.by((): MenuEntry[] => {
    const ref = menuFor;
    if (!ref) return [];
    const tabs = app.openTabs;
    const i = tabs.findIndex((t) => same(t, ref));
    const others = tabs.filter((t) => !same(t, ref));
    const right = tabs.slice(i + 1);
    const saved = tabs.filter((t) => !app.isDirty(t));
    return [
      { id: "close", label: "Close", icon: "close", run: () => app.closeRef(ref) },
      { id: "others", label: "Close others", disabled: others.length === 0, run: () => app.closeRefs(others) },
      { id: "right", label: "Close tabs to the right", disabled: right.length === 0, run: () => app.closeRefs(right) },
      { id: "saved", label: "Close saved tabs", disabled: saved.length === 0, run: () => app.closeRefs(saved) },
      { separator: true },
      { id: "reveal", label: "Reveal in sidebar", icon: "folder-open", disabled: !onreveal, run: () => onreveal?.(ref) },
    ];
  });
</script>

{#if app.openTabs.length > 0}
  <div class="bar" bind:clientWidth={barWidth} style:--tab-min="{TAB_MIN}px" style:--tab-max="{TAB_MAX}px" style:--more-width="{TAB_MORE}px">
    <div class="tabs" role="tablist" aria-label="Open requests" bind:this={listEl}>
      {#each fit.visible as i (`${app.openTabs[i].collection}\u0000${app.openTabs[i].path}`)}
        {@const ref = app.openTabs[i]}
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
          title="{name}{dirty ? ' (unsaved)' : ''} — {collectionName(ref.collection)} › {ref.path}"
          onclick={() => app.selectRef(ref)}
          oncontextmenu={(e) => {
            e.preventDefault();
            openMenu(ref, { x: e.clientX, y: e.clientY });
          }}
          onauxclick={(e) => {
            if (e.button === 1) void app.closeRef(ref);
          }}
          onmousedown={(e) => {
            // No autoscroll cursor on middle click.
            if (e.button === 1) e.preventDefault();
          }}
          onkeydown={(e) => onkeydown(e, i, ref)}
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
    {#if fit.hidden.length > 0}
      <button
        bind:this={moreEl}
        type="button"
        class="more"
        data-testid="tab-more"
        aria-haspopup="dialog"
        aria-expanded={app.tabList}
        aria-label="{fit.hidden.length} more open tabs"
        title="All open tabs ({app.openTabs.length})"
        onclick={() => (app.tabList = !app.tabList)}
      >
        <span class="count">{fit.hidden.length} more</span><Icon name="chevron-down" size="sm" />
      </button>
    {/if}
  </div>
  <TabList bind:open={app.tabList} anchor={moreEl ?? listEl} />
  <Menu items={menuItems} anchor={menuAnchor} bind:open={menuOpen} label="Tab actions" />
{/if}

<style>
  .bar {
    display: flex;
    background: var(--rt-bg-panel);
    border-bottom: 1px solid var(--rt-border);
    overflow: hidden;
  }
  .tabs {
    flex: 1 1 0;
    min-width: 0;
    display: flex;
    overflow: hidden;
  }
  .tab {
    position: relative;
    flex: 1 1 0;
    min-width: var(--tab-min);
    max-width: var(--tab-max);
    display: flex;
    align-items: center;
    gap: var(--rt-space-3);
    box-sizing: border-box;
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
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .where {
    flex: 0 1 auto;
    min-width: 0;
    max-width: 8em;
    overflow: hidden;
    text-overflow: ellipsis;
    font: var(--rt-font-small);
    color: var(--rt-fg-subtle);
  }
  .more {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--rt-space-2);
    width: var(--more-width);
    height: var(--rt-bar-height);
    padding: 0 var(--rt-space-3);
    box-sizing: border-box;
    border: 0;
    background: transparent;
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
    white-space: nowrap;
    cursor: pointer;
  }
  .more:hover,
  .more[aria-expanded="true"] {
    background: var(--rt-bg-hover);
    color: var(--rt-fg);
  }
  .more:focus-visible {
    outline: 2px solid var(--rt-focus);
    outline-offset: -2px;
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
