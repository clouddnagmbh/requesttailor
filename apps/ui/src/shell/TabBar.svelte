<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Open requests as tabs: click to switch, × or middle click to close, a dot for unsaved
     changes. Arrow keys move between tabs (ARIA tabs, automatic activation). The tabs share
     the width (tab-fit.ts); those that do not fit even at the minimum go behind a "N more"
     button that lists all tabs (TabList). Never a horizontal scrollbar. Context menu: close
     variants and "Reveal in sidebar".
     Wave 14 (strand A): tabs of every collection in the sidebar (`AppState.openTabs`); opening
     one makes its collection active. Two tabs with the same name name their collection.
     Tab groups (tab-groups.ts): a coloured chip before the tabs of a group (click collapses,
     menu renames, recolours, ungroups, closes), a coloured underline on its tabs; drag a tab
     onto a tab or chip, drag a chip to move the group; Alt+Left/Right moves the focused tab
     (or chip), Shift+F10 opens the menu. -->
<script lang="ts">
  import { tick } from "svelte";

  import type { RequestRef } from "../commands/types";
  import { getApp } from "../state/app.svelte";
  import { Badge, Icon, IconButton, Menu, type MenuEntry } from "../ui";
  import GroupPopover from "./GroupPopover.svelte";
  import { TAB_MAX, TAB_MIN, TAB_MORE } from "./tab-fit";
  import { chipWidth, COLOR_NAMES, fitGroups, groupLabel, keyOf, type TabGroup } from "./tab-groups";
  import { getTabGroups } from "./tab-groups.svelte";
  import TabList from "./TabList.svelte";
  import { tabElementId } from "./tree";

  let { panelId, onreveal }: { panelId: string; onreveal?: (ref: RequestRef) => unknown } = $props();
  const app = getApp();
  const groups = getTabGroups(app);
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

  const fit = $derived(
    fitGroups({ order: app.openTabs.map(keyOf), state: groups.state, active: app.activeTabIndex, available: barWidth }),
  );

  async function selectAt(i: number) {
    const ref = app.openTabs[i];
    if (!ref) return;
    await app.selectRef(ref);
    await tick();
    listEl?.querySelector<HTMLElement>(`#${tabElementId(i)}`)?.focus();
  }

  /** Focuses the tab (or group chip) after the strip re-rendered. */
  async function focusTab(ref: RequestRef) {
    await tick();
    for (const el of listEl?.querySelectorAll<HTMLElement>('[data-testid="tab"]') ?? []) {
      if (el.dataset.path === ref.path && el.dataset.collection === ref.collection) return el.focus();
    }
  }

  function onkeydown(e: KeyboardEvent, i: number, ref: RequestRef) {
    if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) {
      e.preventDefault();
      openMenu(ref, e.currentTarget as HTMLElement);
      return;
    }
    if (e.target !== e.currentTarget || e.metaKey || e.ctrlKey) return;
    if (e.altKey) {
      // Alt+Left / Alt+Right moves the tab (it leaves or hops over groups, see tab-groups.ts).
      if (e.shiftKey || (e.key !== "ArrowLeft" && e.key !== "ArrowRight")) return;
      e.preventDefault();
      groups.moveTab(ref, e.key === "ArrowLeft" ? -1 : 1);
      void focusTab(ref);
      return;
    }
    // Arrows skip the tabs of collapsed groups.
    const nav = groups.navigable;
    const at = nav.indexOf(i);
    if (at < 0 || nav.length === 0) return;
    const to = { ArrowRight: nav[(at + 1) % nav.length], ArrowLeft: nav[(at - 1 + nav.length) % nav.length], Home: nav[0], End: nav[nav.length - 1] }[e.key];
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
    const own = groups.groupOfRef(ref);
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
      { separator: true },
      { id: "group-new", label: "Add tab to new group", run: () => groups.newGroup(ref) },
      {
        id: "group-add",
        label: "Add tab to group",
        disabled: others.length === 0 || groups.groups.filter((g) => g.id !== own?.id).length === 0,
        items: groups.groups
          .filter((g) => g.id !== own?.id)
          .map((g): MenuEntry => ({ id: `group-add:${g.id}`, label: groupLabel(g), hint: `${groups.refsOf(g.id).length}`, run: () => groups.add(ref, g.id) })),
      },
      ...(own
        ? ([
            { id: "group-open", label: "New tab in group", run: () => groups.openIn(own.id) },
            { id: "group-remove", label: "Remove from group", run: () => groups.remove(ref) },
          ] satisfies MenuEntry[])
        : []),
    ];
  });

  // ---- group chips ----
  let chipOpen = $state(false);
  let chipFor = $state<TabGroup | null>(null);
  let chipAnchor = $state<HTMLElement | { x: number; y: number }>();

  function openChipMenu(group: TabGroup, anchor: HTMLElement | { x: number; y: number }) {
    chipFor = group;
    chipAnchor = anchor;
    chipOpen = true;
  }

  /** The tabs outside a group as menu entries: method and title, headed by collection if several. */
  function addTabEntries(id: string, refs: RequestRef[]): MenuEntry[] {
    const several = new Set(refs.map((r) => r.collection)).size > 1;
    const out: MenuEntry[] = [];
    let last: string | null = null;
    for (const ref of refs) {
      if (several && ref.collection !== last) {
        last = ref.collection;
        out.push({ heading: collectionName(ref.collection) || ref.collection });
      }
      const method = app.draftOf(ref)?.method;
      out.push({ id: `add-tab:${keyOf(ref)}`, label: `${method ? `${method} ` : ""}${label(ref)}`, run: () => groups.add(ref, id) });
    }
    return out;
  }

  const chipItems = $derived.by((): MenuEntry[] => {
    const g = chipFor && groups.group(chipFor.id);
    if (!g) return [];
    const outside = groups.outside(g.id);
    const ungrouped = outside.some((t) => !groups.groupOfRef(t));
    return [
      { id: "rename", label: "Rename…", icon: "edit", run: () => (groups.editing = g.id) },
      {
        id: "color",
        label: "Colour",
        items: COLOR_NAMES.map((name, i): MenuEntry => ({ id: `color:${i}`, label: name, checked: g.color === i, run: () => groups.recolor(g.id, i) })),
      },
      { id: "new-in", label: "New tab in group", icon: "plus", run: () => groups.openIn(g.id) },
      { id: "add-tab", label: "Add tab", disabled: outside.length === 0, items: addTabEntries(g.id, outside) },
      { id: "add-all", label: "Add all ungrouped tabs", disabled: !ungrouped, run: () => groups.addAllUngrouped(g.id) },
      { id: "toggle", label: g.collapsed ? "Expand group" : "Collapse group", run: () => groups.toggle(g.id) },
      { separator: true },
      { id: "ungroup", label: "Ungroup", run: () => groups.ungroup(g.id) },
      { id: "close-group", label: "Close group", icon: "close", danger: true, run: () => groups.closeGroup(g.id) },
    ];
  });

  function onchipkey(e: KeyboardEvent, group: TabGroup) {
    const el = e.currentTarget as HTMLElement;
    if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) {
      e.preventDefault();
      openChipMenu(group, el);
    } else if (e.key === "F2") {
      e.preventDefault();
      groups.editing = group.id;
    } else if (e.altKey && !e.shiftKey && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
      e.preventDefault();
      groups.moveGroup(group.id, e.key === "ArrowLeft" ? -1 : 1);
      tick().then(() => chipEl(group.id)?.focus());
    }
  }

  const chipEl = (id: string) => listEl?.querySelector<HTMLElement>(`[data-testid="group-chip"][data-group="${id}"]`) ?? undefined;
  const editingGroup = $derived(groups.editing ? groups.group(groups.editing) : undefined);
  let editAnchor = $state<HTMLElement>();
  $effect(() => {
    const id = groups.editing;
    void fit;
    if (!id) {
      editAnchor = undefined;
      return;
    }
    tick().then(() => (editAnchor = chipEl(id)));
  });
  function closeEditor() {
    const id = groups.editing;
    groups.editing = null;
    if (id) tick().then(() => chipEl(id)?.focus());
  }

  // ---- drag and drop ----
  let mark = $state<{ key: string; after: boolean } | { group: string } | null>(null);
  const draggingTab = (e: DragEvent) => groups.drag !== null && e.dataTransfer !== null;
  const afterHalf = (e: DragEvent) => {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    return e.clientX > r.left + r.width / 2;
  };

  function dragStart(e: DragEvent, drag: NonNullable<typeof groups.drag>, text: string) {
    groups.drag = drag;
    e.dataTransfer?.setData("text/plain", text);
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
  }
  function dragEnd() {
    groups.drag = null;
    mark = null;
  }
  function overTab(e: DragEvent, key: string) {
    if (!draggingTab(e)) return;
    e.preventDefault();
    e.stopPropagation();
    mark = { key, after: afterHalf(e) };
  }
  function dropOnTab(e: DragEvent, key: string) {
    const drag = groups.drag;
    if (!drag) return;
    e.preventDefault();
    e.stopPropagation();
    const after = afterHalf(e);
    dragEnd();
    if (drag.kind === "tab") groups.dropTab(drag.key, key, after);
    else groups.dropGroup(drag.id, key, after);
  }
  function overChip(e: DragEvent, group: TabGroup) {
    if (!draggingTab(e)) return;
    e.preventDefault();
    e.stopPropagation();
    mark = groups.drag?.kind === "tab" ? { group: group.id } : { key: keyOf(groups.refsOf(group.id)[0]), after: afterHalf(e) };
  }
  function dropOnChip(e: DragEvent, group: TabGroup) {
    const drag = groups.drag;
    if (!drag) return;
    e.preventDefault();
    e.stopPropagation();
    const after = afterHalf(e);
    dragEnd();
    if (drag.kind === "tab") groups.dropTabOnGroup(drag.key, group.id);
    else if (drag.id !== group.id) groups.dropGroup(drag.id, keyOf(groups.refsOf(group.id)[0]), after);
  }
  function overStrip(e: DragEvent) {
    if (!draggingTab(e)) return;
    e.preventDefault();
    mark = null;
  }
  function dropOnStrip(e: DragEvent) {
    const drag = groups.drag;
    if (!drag) return;
    e.preventDefault();
    dragEnd();
    if (drag.kind === "tab") groups.dropTabAtEnd(drag.key);
    else groups.dropGroupAtEnd(drag.id);
  }
  const marked = (key: string) => (mark && "key" in mark && mark.key === key ? (mark.after ? "after" : "before") : undefined);
</script>

{#if app.openTabs.length > 0}
  <div class="bar" bind:clientWidth={barWidth} style:--tab-min="{TAB_MIN}px" style:--tab-max="{TAB_MAX}px" style:--more-width="{TAB_MORE}px">
    <!-- Dropping on the empty part of the strip: svelte-ignore keeps the tablist role. -->
    <!-- svelte-ignore a11y_interactive_supports_focus -->
    <div class="tabs" role="tablist" aria-label="Open requests" bind:this={listEl} ondragover={overStrip} ondrop={dropOnStrip}>
      {#each fit.items as item (item.kind === "chip" ? `chip:${item.group.id}` : item.key)}
       {#if item.kind === "chip"}
        {@const group = item.group}
        {@const n = item.keys.length}
        <button
          type="button"
          class="chip"
          class:collapsed={group.collapsed}
          class:drop={mark !== null && "group" in mark && mark.group === group.id}
          class:before={marked(item.keys[0]) === "before"}
          class:after={marked(item.keys[0]) === "after"}
          style:--grp="var(--rt-lane-{group.color + 1})"
          style:width="{chipWidth(group, n)}px"
          data-testid="group-chip"
          data-group={group.id}
          data-collapsed={group.collapsed}
          aria-expanded={!group.collapsed}
          aria-haspopup="menu"
          aria-label="{groupLabel(group)}, {n} {n === 1 ? 'tab' : 'tabs'}, {group.collapsed ? 'collapsed' : 'expanded'}"
          title="{groupLabel(group)} — click to {group.collapsed ? 'expand' : 'collapse'}"
          draggable="true"
          onclick={() => groups.toggle(group.id)}
          oncontextmenu={(e) => {
            e.preventDefault();
            openChipMenu(group, { x: e.clientX, y: e.clientY });
          }}
          onkeydown={(e) => onchipkey(e, group)}
          ondragstart={(e) => dragStart(e, { kind: "group", id: group.id }, groupLabel(group))}
          ondragend={dragEnd}
          ondragover={(e) => overChip(e, group)}
          ondragleave={() => (mark = null)}
          ondrop={(e) => dropOnChip(e, group)}
        >
          <span class="swatch" aria-hidden="true"></span>
          {#if group.name}<span class="chip-name" data-testid="group-name-label">{group.name}</span>{/if}
          {#if group.collapsed}<span class="chip-count" data-testid="group-count">{n}</span>{/if}
        </button>
       {:else}
        {@const i = item.index}
        {@const ref = app.openTabs[i]}
        {@const grp = item.group}
        {@const selected = isSelected(ref)}
        {@const dirty = app.isDirty(ref)}
        {@const draft = app.draftOf(ref)}
        {@const name = label(ref)}
        {@const where = ambiguous.has(name) ? collectionName(ref.collection) : ""}
        <div
          class="tab"
          class:selected
          class:dirty
          class:grouped={grp !== undefined}
          class:before={marked(item.key) === "before"}
          class:after={marked(item.key) === "after"}
          style:--grp={grp ? `var(--rt-lane-${grp.color + 1})` : undefined}
          data-group={grp?.id}
          draggable="true"
          ondragstart={(e) => dragStart(e, { kind: "tab", key: item.key }, name)}
          ondragend={dragEnd}
          ondragover={(e) => overTab(e, item.key)}
          ondragleave={() => (mark = null)}
          ondrop={(e) => dropOnTab(e, item.key)}
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
       {/if}
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
  <Menu items={chipItems} anchor={chipAnchor} bind:open={chipOpen} label="Group actions" />
  {#if editingGroup && editAnchor}
    <GroupPopover
      group={editingGroup}
      anchor={editAnchor}
      onrename={(name) => groups.rename(editingGroup.id, name)}
      onrecolor={(c) => groups.recolor(editingGroup.id, c)}
      onclose={closeEditor}
    />
  {/if}
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
  /* Tab groups: a coloured underline on the tabs, a chip before them. */
  .tab.grouped {
    box-shadow: inset 0 -2px 0 var(--grp);
  }
  .tab.before::after,
  .tab.after::after,
  .chip.before::after,
  .chip.after::after {
    content: "";
    position: absolute;
    top: 4px;
    bottom: 4px;
    width: 2px;
    background: var(--rt-accent-solid);
  }
  .tab.before::after,
  .chip.before::after {
    left: 0;
  }
  .tab.after::after,
  .chip.after::after {
    right: 0;
  }
  .chip {
    position: relative;
    flex: none;
    align-self: center;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--rt-space-2);
    box-sizing: border-box;
    height: var(--rt-control-sm);
    margin: 0 var(--rt-space-2);
    padding: 0 var(--rt-space-3);
    border: 1px solid var(--grp);
    border-radius: var(--rt-radius-pill);
    background: color-mix(in srgb, var(--grp) 24%, var(--rt-bg-panel));
    color: var(--rt-fg);
    font: var(--rt-font-small);
    white-space: nowrap;
    cursor: pointer;
    user-select: none;
  }
  .chip:hover {
    background: color-mix(in srgb, var(--grp) 38%, var(--rt-bg-panel));
  }
  .chip.drop {
    background: color-mix(in srgb, var(--grp) 55%, var(--rt-bg-panel));
  }
  .chip:focus-visible {
    outline: 2px solid var(--rt-focus);
    outline-offset: 1px;
  }
  .swatch {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: var(--rt-radius-pill);
    background: var(--grp);
  }
  .chip-name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .chip-count {
    flex: none;
    font-variant-numeric: tabular-nums;
    color: var(--rt-fg-muted);
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
