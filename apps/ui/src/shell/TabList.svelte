<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- All open tabs in a popover under the tab strip ("N more", "Go to tab…"): grouped by
     collection, type to filter, Up/Down choose, Enter opens, Escape closes (focus goes back to
     where it was). Each row has the method badge, an unsaved dot and a close button. -->
<script lang="ts">
  import { tick } from "svelte";

  import type { RequestRef } from "../commands/types";
  import { getApp } from "../state/app.svelte";
  import { Badge, IconButton } from "../ui";
  import { portal, rectOf, viewport } from "../ui/floating";
  import { place } from "../ui/position";
  import { fuzzyFilter } from "./fuzzy";

  let { open = $bindable(false), anchor }: { open?: boolean; anchor: HTMLElement | undefined } = $props();
  const app = getApp();

  let query = $state("");
  let index = $state(0);
  let el = $state<HTMLElement>();
  let pos = $state<{ x: number; y: number } | null>(null);
  let returnFocus: HTMLElement | null = null;

  const nameOf = (ref: RequestRef) => app.draftOf(ref)?.name ?? ref.path.split("/").pop() ?? ref.path;
  const collectionName = (id: string) => app.collections.find((c) => c.id === id)?.name ?? "";
  const same = (a: RequestRef, b: RequestRef) => a.collection === b.collection && a.path === b.path;

  const shown = $derived(fuzzyFilter(app.openTabs, query, (r) => `${nameOf(r)} ${collectionName(r.collection)} ${r.path}`));
  /** The shown tabs in groups, in tab order of their first tab. */
  const groups = $derived.by(() => {
    const out: { id: string; name: string; refs: RequestRef[] }[] = [];
    for (const r of shown) {
      let g = out.find((x) => x.id === r.collection);
      if (!g) out.push((g = { id: r.collection, name: collectionName(r.collection), refs: [] }));
      g.refs.push(r);
    }
    return out;
  });
  const flat = $derived(groups.flatMap((g) => g.refs));
  const current = $derived(flat[Math.min(index, flat.length - 1)]);

  $effect.pre(() => {
    if (!open) return;
    returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    query = "";
    // Start on the shown tab.
    index = Math.max(0, app.activeTabIndex);
  });

  $effect(() => {
    if (!open || !el || !anchor) return;
    void flat.length;
    const r = el.getBoundingClientRect();
    pos = place(rectOf(anchor), { width: r.width, height: r.height }, viewport(), "bottom-end", 2);
  });

  $effect(() => {
    if (!open) return;
    const down = (e: PointerEvent) => {
      const t = e.target as Element | null;
      if (el?.contains(t) || anchor?.contains(t)) return;
      void close(false);
    };
    document.addEventListener("pointerdown", down, true);
    return () => {
      document.removeEventListener("pointerdown", down, true);
      pos = null;
    };
  });

  async function close(restore = true) {
    open = false;
    await tick();
    if (restore && returnFocus?.isConnected) returnFocus.focus();
    returnFocus = null;
  }

  async function choose(ref: RequestRef | undefined) {
    if (!ref) return;
    await close(false);
    await app.selectRef(ref);
    await tick();
    // Focus the tab that is now shown (it may have taken the last slot of the strip).
    document.querySelector<HTMLElement>('[data-testid="tab"][aria-selected="true"]')?.focus();
  }

  async function closeTab(ref: RequestRef) {
    await app.closeRef(ref);
    if (app.openTabs.length === 0) return close();
    await tick();
    input?.focus();
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "ArrowDown") index = Math.min(index + 1, flat.length - 1);
    else if (e.key === "ArrowUp") index = Math.max(index - 1, 0);
    else if (e.key === "Home" && e.ctrlKey) index = 0;
    else if (e.key === "Enter") void choose(current);
    else if (e.key === "Escape") void close();
    else if (e.key === "Tab") void close(false);
    else return;
    e.preventDefault();
    e.stopPropagation();
    tick().then(() => el?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" }));
  }

  let input = $state<HTMLInputElement>();
  // After the portal moved the popover to <body> (moving a node drops its focus).
  $effect(() => {
    if (open && input) tick().then(() => input?.focus());
  });
</script>

{#if open}
  <div
    bind:this={el}
    use:portal={anchor}
    class="tab-list"
    class:placed={pos !== null}
    role="dialog"
    tabindex="-1"
    aria-label="Open tabs"
    data-testid="tab-list"
    {onkeydown}
    style:left="{pos?.x ?? 0}px"
    style:top="{pos?.y ?? 0}px"
  >
    <input
      role="combobox"
      aria-expanded="true"
      aria-controls="rt-tab-list-options"
      aria-autocomplete="list"
      aria-activedescendant={current ? `rt-tab-opt-${flat.indexOf(current)}` : undefined}
      aria-label="Filter tabs"
      placeholder="Filter tabs"
      data-testid="tab-list-filter"
      bind:value={query}
      oninput={() => (index = 0)}
      bind:this={input}
    />
    <div id="rt-tab-list-options" class="options" role="listbox" aria-label="Open tabs">
      {#each groups as group (group.id)}
        <div class="group" role="presentation">{group.name}</div>
        {#each group.refs as ref (ref.path)}
          {@const i = flat.indexOf(ref)}
          {@const draft = app.draftOf(ref)}
          {@const isCurrent = ref === current}
          <!-- Options are chosen with the arrow keys in the input (aria-activedescendant). -->
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div
            id="rt-tab-opt-{i}"
            class="row"
            class:current={isCurrent}
            class:shown={app.activeTabIndex >= 0 && same(app.openTabs[app.activeTabIndex], ref)}
            role="option"
            tabindex="-1"
            aria-selected={isCurrent}
            data-testid="tab-list-row"
            data-path={ref.path}
            title="{collectionName(ref.collection)} › {ref.path}"
            onclick={() => choose(ref)}
            onpointermove={() => (index = i)}
          >
            {#if draft}<span class="method"><Badge kind="method" value={draft.method} /></span>{/if}
            <span class="name">{nameOf(ref)}</span>
            {#if app.isDirty(ref)}<span class="dot" data-testid="tab-list-dirty" role="img" aria-label="unsaved"></span>{/if}
            <span class="close" role="none" onclick={(e) => e.stopPropagation()}>
              <IconButton icon="close" size="sm" tabindex={-1} label="Close {nameOf(ref)}" onclick={() => closeTab(ref)} />
            </span>
          </div>
        {/each}
      {:else}
        <p class="none">No matching tab</p>
      {/each}
    </div>
  </div>
{/if}

<style>
  .tab-list {
    position: fixed;
    z-index: var(--rt-z-popover);
    display: flex;
    flex-direction: column;
    width: 320px;
    max-width: calc(100vw - 16px);
    max-height: min(440px, calc(100vh - 16px));
    box-sizing: border-box;
    padding: var(--rt-space-2);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-lg);
    background: var(--rt-surface-3);
    color: var(--rt-fg);
    box-shadow: var(--rt-elev-2);
    font: var(--rt-font-body);
    visibility: hidden;
  }
  .placed {
    visibility: visible;
  }
  input {
    flex: none;
    height: var(--rt-control-md);
    margin-bottom: var(--rt-space-2);
    padding: 0 var(--rt-space-4);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-md);
    background: var(--rt-bg);
    color: inherit;
    font: inherit;
  }
  input:focus-visible {
    outline: none;
    box-shadow: var(--rt-focus-ring);
  }
  .options {
    overflow-y: auto;
    min-height: 0;
  }
  .group {
    padding: var(--rt-space-3) var(--rt-space-4) var(--rt-space-2);
    color: var(--rt-fg-subtle);
    font: var(--rt-font-caption);
    letter-spacing: var(--rt-tracking-caps);
    text-transform: uppercase;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--rt-space-3);
    height: var(--rt-control-md);
    padding: 0 var(--rt-space-2) 0 var(--rt-space-4);
    border-radius: var(--rt-radius-md);
    cursor: default;
  }
  .row.current {
    background: var(--rt-bg-hover);
  }
  .row.shown .name {
    font-weight: 600;
  }
  .method {
    flex: none;
    display: inline-flex;
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: var(--rt-radius-pill);
    background: var(--rt-accent);
  }
  .close {
    flex: none;
    display: inline-flex;
  }
  .none {
    margin: 0;
    padding: var(--rt-space-4);
    color: var(--rt-fg-subtle);
  }
</style>
