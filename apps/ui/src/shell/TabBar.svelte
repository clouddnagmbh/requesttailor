<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Open requests as tabs: click to switch, × or middle click to close, a dot for unsaved
     changes. Arrow keys move between tabs (ARIA tabs, automatic activation). -->
<script lang="ts">
  import { tick } from "svelte";

  import { getApp } from "../state/app.svelte";
  import MethodBadge from "./MethodBadge.svelte";
  import { tabElementId } from "./tree";

  let { panelId }: { panelId: string } = $props();
  const app = getApp();
  let listEl = $state<HTMLElement>();

  const label = (path: string) => app.drafts[path]?.name ?? path.split("/").pop() ?? path;

  const tabId = (path: string) => tabElementId(app.tabs.indexOf(path));

  async function selectAt(i: number) {
    const path = app.tabs[i];
    if (!path) return;
    await app.select(path);
    await tick();
    listEl?.querySelector<HTMLElement>(`#${tabId(path)}`)?.focus();
  }

  function onkeydown(e: KeyboardEvent, i: number) {
    if (e.target !== e.currentTarget || e.metaKey || e.ctrlKey || e.altKey) return;
    const last = app.tabs.length - 1;
    const to = { ArrowRight: i === last ? 0 : i + 1, ArrowLeft: i === 0 ? last : i - 1, Home: 0, End: last }[e.key];
    if (to === undefined) return;
    e.preventDefault();
    selectAt(to);
  }
</script>

{#if app.tabs.length > 0}
  <div class="tabs" role="tablist" aria-label="Open requests" bind:this={listEl}>
    {#each app.tabs as path, i (path)}
      {@const selected = path === app.active}
      {@const dirty = app.dirty.includes(path)}
      <div
        class="tab"
        class:selected
        role="tab"
        id={tabId(path)}
        data-testid="tab"
        data-path={path}
        aria-selected={selected}
        aria-controls={panelId}
        aria-label={dirty ? `${label(path)} (unsaved)` : label(path)}
        tabindex={selected ? 0 : -1}
        title={path}
        onclick={() => app.select(path)}
        onauxclick={(e) => {
          if (e.button === 1) app.close(path);
        }}
        onmousedown={(e) => {
          // No autoscroll cursor on middle click.
          if (e.button === 1) e.preventDefault();
        }}
        onkeydown={(e) => onkeydown(e, i)}
      >
        {#if app.drafts[path]}<MethodBadge method={app.drafts[path].method} />{/if}
        <span class="label">{label(path)}</span>
        {#if dirty}<span class="dirty" data-testid="tab-dirty" aria-hidden="true">●</span>{/if}
        <button
          class="close"
          tabindex="-1"
          aria-label="Close {label(path)}"
          title="Close"
          onclick={(e) => {
            e.stopPropagation();
            app.close(path);
          }}>×</button
        >
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
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
    max-width: 220px;
    height: 32px;
    padding: 0 4px 0 12px;
    border-right: 1px solid var(--rt-border);
    color: var(--rt-fg-muted);
    cursor: default;
    user-select: none;
    white-space: nowrap;
  }
  .tab:hover {
    background: var(--rt-bg-hover);
  }
  .tab.selected {
    background: var(--rt-bg);
    color: var(--rt-fg);
    box-shadow: inset 0 2px 0 var(--rt-accent);
  }
  .tab:focus-visible {
    outline: 2px solid var(--rt-focus);
    outline-offset: -2px;
  }
  .label {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .dirty {
    color: var(--rt-accent);
    font-size: 10px;
  }
  .close {
    border: 0;
    background: transparent;
    color: var(--rt-fg-muted);
    font: inherit;
    font-size: 15px;
    line-height: 1;
    width: 20px;
    height: 20px;
    border-radius: var(--rt-radius);
    cursor: pointer;
  }
  .close:hover {
    background: var(--rt-bg-hover);
    color: var(--rt-fg);
  }
</style>
