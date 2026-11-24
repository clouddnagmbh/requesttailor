<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Rename and colour of a tab group, in a small popover under its chip (opens at once for a
     new group, as in Chrome). Type a name, Enter or Escape closes; arrows choose the colour. -->
<script lang="ts">
  import { tick } from "svelte";

  import { portal, rectOf, viewport } from "../ui/floating";
  import { place } from "../ui/position";
  import { COLOR_NAMES, GROUP_COLORS, type TabGroup } from "./tab-groups";

  let {
    group,
    anchor,
    onrename,
    onrecolor,
    onclose,
  }: {
    group: TabGroup;
    anchor: HTMLElement | undefined;
    onrename: (name: string) => void;
    onrecolor: (color: number) => void;
    onclose: () => void;
  } = $props();

  let el = $state<HTMLElement>();
  let input = $state<HTMLInputElement>();
  let pos = $state<{ x: number; y: number } | null>(null);
  // The name is typed here; the group only supplies the start value.
  // svelte-ignore state_referenced_locally
  let name = $state(group.name);

  $effect(() => {
    if (!el || !anchor) return;
    const r = el.getBoundingClientRect();
    pos = place(rectOf(anchor), { width: r.width, height: r.height }, viewport(), "bottom-start", 2);
  });

  // After the portal moved the node (moving drops focus).
  $effect(() => {
    if (input) tick().then(() => (input?.focus(), input?.select()));
  });

  $effect(() => {
    const down = (e: PointerEvent) => {
      const t = e.target as Element | null;
      if (el?.contains(t) || anchor?.contains(t)) return;
      onclose();
    };
    document.addEventListener("pointerdown", down, true);
    return () => document.removeEventListener("pointerdown", down, true);
  });

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Enter" || e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onclose();
    }
  }

  function onswatch(e: KeyboardEvent, i: number) {
    const to = { ArrowRight: i + 1, ArrowDown: i + 1, ArrowLeft: i - 1, ArrowUp: i - 1 }[e.key];
    if (to === undefined) return;
    e.preventDefault();
    const next = (to + GROUP_COLORS) % GROUP_COLORS;
    onrecolor(next);
    tick().then(() => el?.querySelectorAll<HTMLElement>('[role="radio"]')[next]?.focus());
  }
</script>

<div
  bind:this={el}
  use:portal={anchor}
  class="popover"
  class:placed={pos !== null}
  role="dialog"
  tabindex="-1"
  aria-label="Tab group"
  data-testid="group-popover"
  {onkeydown}
  style:left="{pos?.x ?? 0}px"
  style:top="{pos?.y ?? 0}px"
>
  <input
    bind:this={input}
    bind:value={name}
    aria-label="Group name"
    placeholder="Name this group"
    data-testid="group-name"
    maxlength="60"
    oninput={() => onrename(name)}
  />
  <div class="swatches" role="radiogroup" aria-label="Group colour">
    {#each { length: GROUP_COLORS } as _, i (i)}
      <button
        type="button"
        class="swatch"
        role="radio"
        aria-checked={group.color === i}
        aria-label={COLOR_NAMES[i]}
        title={COLOR_NAMES[i]}
        tabindex={group.color === i ? 0 : -1}
        data-testid="group-color"
        data-color={i}
        style:--swatch="var(--rt-lane-{i + 1})"
        onclick={() => onrecolor(i)}
        onkeydown={(e) => onswatch(e, i)}
      ></button>
    {/each}
  </div>
</div>

<style>
  .popover {
    position: fixed;
    z-index: var(--rt-z-popover);
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-4);
    width: 240px;
    box-sizing: border-box;
    padding: var(--rt-space-4);
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
    height: var(--rt-control-md);
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
  .swatches {
    display: flex;
    gap: var(--rt-space-3);
  }
  .swatch {
    flex: 1;
    height: 20px;
    padding: 0;
    border: 2px solid transparent;
    border-radius: var(--rt-radius-pill);
    background: var(--swatch);
    cursor: pointer;
  }
  .swatch[aria-checked="true"] {
    border-color: var(--rt-fg);
  }
  .swatch:focus-visible {
    outline: 2px solid var(--rt-focus);
    outline-offset: 2px;
  }
</style>
