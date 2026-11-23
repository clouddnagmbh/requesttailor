<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- A dropdown menu of the Git panel: a trigger and a list of items (role="menu"), arrow keys,
     Enter, Escape (focus back to the trigger), closes on a click outside. -->
<script lang="ts" module>
  export interface MenuItem {
    id: string;
    label: string;
    run?: () => unknown;
    /** A heading above the following items (not selectable). */
    heading?: boolean;
    separator?: boolean;
    disabled?: boolean;
    checked?: boolean;
    /** Muted text on the right ("origin"). */
    hint?: string;
    title?: string;
    danger?: boolean;
  }
</script>

<script lang="ts">
  import { tick, type Snippet } from "svelte";

  let {
    items,
    label,
    testid,
    trigger,
    align = "start",
    triggerClass = "",
    disabled = false,
    title,
  }: {
    items: MenuItem[];
    /** Accessible name of the trigger. */
    label: string;
    testid: string;
    trigger: Snippet;
    align?: "start" | "end";
    triggerClass?: string;
    disabled?: boolean;
    title?: string;
  } = $props();

  let open = $state(false);
  let button = $state<HTMLButtonElement>();
  let list = $state<HTMLDivElement>();
  let root = $state<HTMLDivElement>();

  const entries = () => [...(list?.querySelectorAll<HTMLButtonElement>('[role^="menuitem"]:not(:disabled)') ?? [])];

  async function show(focusLast = false) {
    open = true;
    await tick();
    const all = entries();
    (focusLast ? all.at(-1) : all[0])?.focus();
  }

  function hide(refocus = true) {
    open = false;
    if (refocus) button?.focus();
  }

  function onkeydown(e: KeyboardEvent) {
    const all = entries();
    const at = all.indexOf(document.activeElement as HTMLButtonElement);
    if (e.key === "ArrowDown") {
      e.preventDefault();
      all[(at + 1) % all.length]?.focus();
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      all[(at - 1 + all.length) % all.length]?.focus();
    } else if (e.key === "Home") {
      e.preventDefault();
      all[0]?.focus();
    } else if (e.key === "End") {
      e.preventDefault();
      all.at(-1)?.focus();
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      hide();
    } else if (e.key === "Tab") {
      hide(false);
    }
  }

  function outside(e: PointerEvent) {
    if (open && root && !root.contains(e.target as Node)) hide(false);
  }

  function pick(item: MenuItem) {
    hide();
    void item.run?.();
  }
</script>

<svelte:window onpointerdown={outside} />

<div class="menu-root" bind:this={root}>
  <button
    bind:this={button}
    class={triggerClass}
    data-testid={testid}
    aria-label={label}
    aria-haspopup="menu"
    aria-expanded={open}
    {title}
    {disabled}
    onclick={() => (open ? hide() : void show())}
    onkeydown={(e) => {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        void show();
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        void show(true);
      }
    }}
  >
    {@render trigger()}
  </button>
  {#if open}
    <div class="list {align}" role="menu" tabindex="-1" aria-label={label} data-testid="{testid}-list" bind:this={list} {onkeydown}>
      {#each items as item (item.id)}
        {#if item.separator}
          <div class="sep" role="separator"></div>
        {:else if item.heading}
          <div class="heading" role="presentation">{item.label}</div>
        {:else}
          <button
            role={item.checked === undefined ? "menuitem" : "menuitemradio"}
            aria-checked={item.checked}
            class="item"
            class:danger={item.danger}
            data-testid="{testid}-{item.id}"
            disabled={item.disabled}
            title={item.title}
            onclick={() => pick(item)}
          >
            <span class="check" aria-hidden="true">{item.checked ? "✓" : ""}</span>
            <span class="label">{item.label}</span>
            {#if item.hint}<span class="hint">{item.hint}</span>{/if}
          </button>
        {/if}
      {/each}
    </div>
  {/if}
</div>

<style>
  .menu-root {
    position: relative;
    display: inline-flex;
    min-width: 0;
  }
  .list {
    position: absolute;
    top: calc(100% + 4px);
    z-index: 30;
    min-width: 200px;
    max-width: min(320px, calc(100vw - 24px));
    max-height: 60vh;
    overflow: auto;
    padding: 4px;
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    box-shadow: var(--rt-shadow);
  }
  .list.start {
    left: 0;
  }
  .list.end {
    right: 0;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 4px 8px 4px 4px;
    border: none;
    border-radius: var(--rt-radius);
    background: none;
    color: var(--rt-fg);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .item:hover:not(:disabled),
  .item:focus-visible {
    background: var(--rt-bg-hover);
    outline: none;
  }
  .item:disabled {
    color: var(--rt-fg-muted);
    cursor: default;
  }
  .item.danger {
    color: var(--rt-danger);
  }
  .check {
    width: 14px;
    flex: none;
    text-align: center;
    color: var(--rt-accent);
  }
  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .hint {
    flex: none;
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
  .heading {
    padding: 6px 8px 2px 24px;
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
    font-weight: 600;
  }
  .sep {
    height: 1px;
    margin: 4px 0;
    background: var(--rt-border);
  }
</style>
