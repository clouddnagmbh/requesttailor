<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The sequence diagram as an own SVG (wave 19, strand G): no library, deterministic layout
     (`layout.ts`), only the rows in the window are drawn, so 2 000 messages scroll smoothly.
     Keyboard: Up/Down (or j/k) move from message to message, Home/End jump, Enter opens the
     request of the message, c copies the Markdown. The participants stay in view at the top. -->
<script lang="ts">
  import type { SequenceModel } from "../commands/types";
  import { fitLabel, layoutSequence, ROW_HEIGHT, visibleGroups, visibleItems } from "./layout";
  import { describeSelection, moveSelection } from "./model";

  let {
    model,
    selected = $bindable(null),
    onopen,
    oncopy,
  }: {
    model: SequenceModel;
    selected?: number | null;
    onopen?: (requestPath: string) => void;
    oncopy?: () => void;
  } = $props();

  let scroller = $state<HTMLDivElement>();
  let top = $state(0);
  let viewHeight = $state(600);
  let raf = 0;

  const layout = $derived(layoutSequence(model));
  const items = $derived(visibleItems(layout, top, viewHeight));
  const groups = $derived(visibleGroups(layout, top, viewHeight));

  function onscroll() {
    if (raf) return;
    raf = requestAnimationFrame(() => {
      raf = 0;
      if (scroller) top = scroller.scrollTop;
    });
  }

  $effect(() => {
    const el = scroller;
    if (!el) return;
    viewHeight = el.clientHeight || 600;
    const ro = new ResizeObserver(() => {
      viewHeight = el.clientHeight || 600;
    });
    ro.observe(el);
    return () => {
      ro.disconnect();
      if (raf) cancelAnimationFrame(raf);
    };
  });

  function reveal(index: number) {
    const el = scroller;
    const y = layout.messageY[index];
    if (!el || y === undefined) return;
    const head = 56;
    if (y < el.scrollTop) el.scrollTop = Math.max(0, y);
    else if (y + ROW_HEIGHT > el.scrollTop + el.clientHeight - head) el.scrollTop = y + ROW_HEIGHT - el.clientHeight + head + 8;
    top = el.scrollTop;
  }

  function select(next: number | null) {
    selected = next;
    if (next !== null) reveal(next);
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const count = model.messages.length;
    const page = Math.max(1, Math.floor(viewHeight / ROW_HEIGHT) - 1);
    switch (e.key) {
      case "ArrowDown":
      case "j":
        select(moveSelection(selected, 1, count));
        break;
      case "ArrowUp":
      case "k":
        select(moveSelection(selected, -1, count));
        break;
      case "PageDown":
        select(moveSelection(selected, page, count));
        break;
      case "PageUp":
        select(moveSelection(selected, -page, count));
        break;
      case "Home":
        select(count ? 0 : null);
        break;
      case "End":
        select(count ? count - 1 : null);
        break;
      case "Enter": {
        const path = selected === null ? undefined : model.messages[selected]?.request_path;
        if (!path) return;
        onopen?.(path);
        break;
      }
      case "c":
        oncopy?.();
        break;
      default:
        return;
    }
    e.preventDefault();
  }

  const spoken = $derived(describeSelection(model, selected));
</script>

<!-- A scrollable region with its own keyboard handling (arrows between messages): focusable on purpose. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="scroller"
  bind:this={scroller}
  {onscroll}
  {onkeydown}
  tabindex="0"
  role="group"
  aria-label="Sequence diagram"
  aria-describedby="sequence-live"
  data-testid="sequence-diagram"
>
  <div class="sr" id="sequence-live" aria-live="polite" data-testid="sequence-live">{spoken}</div>
  <div class="heads" style:width="{layout.width}px">
    {#each layout.columns as column (column.participant.id)}
      <div class="head" data-kind={column.participant.kind} style:left="{column.x}px" title={column.participant.label} data-testid="sequence-participant">
        {column.participant.label}
      </div>
    {/each}
  </div>
  <svg width={layout.width} height={layout.height} role="img" aria-label="{model.messages.length} messages" data-testid="sequence-svg">
    <defs>
      <marker id="seq-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
        <path d="M0 0 L10 5 L0 10 z" fill="context-stroke" />
      </marker>
    </defs>
    {#each layout.columns as column (column.participant.id)}
      <line class="life" x1={column.x} x2={column.x} y1="0" y2={layout.height} />
    {/each}
    {#each groups as g, i (i + ":" + g.y1 + ":" + g.depth)}
      <g class="group" data-kind={g.kind}>
        <rect x={g.x1} y={g.y1 + 2} width={g.x2 - g.x1} height={Math.max(8, g.y2 - g.y1 - 4)} rx="6" />
        <text x={g.x1 + 8} y={g.y1 + 14}>{g.kind === "rect" ? g.label : `${g.kind} ${g.label}`}</text>
      </g>
    {/each}
    {#each items as item (item.type + ":" + (item.type === "message" ? item.index : item.after + ":" + item.y))}
      {#if item.type === "message"}
        {@const span = Math.abs(item.x2 - item.x1)}
        {@const mid = (item.x1 + item.x2) / 2}
        <g
          class="msg"
          class:response={item.kind === "response"}
          class:selected={selected === item.index}
          data-testid="sequence-message"
          data-index={item.index}
          data-selected={selected === item.index ? "true" : undefined}
          role="presentation"
          onclick={() => select(item.index)}
          ondblclick={() => item.requestPath && onopen?.(item.requestPath)}
        >
          <title>{item.label}{item.requestPath ? ` (${item.requestPath})` : ""}</title>
          <rect class="hit" x="0" y={item.y} width={layout.width} height={item.height} />
          {#if item.self}
            <path class="arrow" d="M{item.x1} {item.y + 12} h28 v14 h-28" marker-end="url(#seq-arrow)" fill="none" />
            <text class="label" x={item.x1 + 34} y={item.y + 22}>{fitLabel(item.label, 160)}</text>
          {:else}
            <line class="arrow" x1={item.x1} x2={item.x2} y1={item.y + 26} y2={item.y + 26} marker-end="url(#seq-arrow)" />
            <text class="label" x={mid} y={item.y + 20} text-anchor="middle">{fitLabel(item.label, span)}</text>
          {/if}
        </g>
      {:else}
        <g class="note" data-testid="sequence-note">
          <title>{item.text}</title>
          <rect x={item.x} y={item.y + 3} width={item.width} height={item.height - 6} rx="4" />
          <text x={item.x + 8} y={item.y + 19}>{fitLabel(item.text, item.width)}</text>
        </g>
      {/if}
    {/each}
  </svg>
</div>

<style>
  .scroller {
    position: relative;
    flex: 1;
    min-height: 0;
    overflow: auto;
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-md);
  }
  .scroller:focus-visible {
    outline: none;
    box-shadow: var(--rt-focus-ring);
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }
  .heads {
    position: sticky;
    top: 0;
    z-index: 2;
    height: 44px;
    background: var(--rt-bg-panel);
    border-bottom: 1px solid var(--rt-border);
  }
  .head {
    position: absolute;
    top: 8px;
    transform: translateX(-50%);
    max-width: 176px;
    box-sizing: border-box;
    padding: 4px 10px;
    border: 1px solid var(--rt-border-strong);
    border-radius: var(--rt-radius-sm);
    background: var(--rt-bg);
    color: var(--rt-fg);
    font: var(--rt-font-small);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .head[data-kind="client"] {
    border-color: var(--rt-accent);
    color: var(--rt-accent-subtle-fg);
    background: var(--rt-accent-subtle);
  }
  .head[data-kind="agent"] {
    border-color: var(--rt-lane-4, var(--rt-border-strong));
  }
  svg {
    display: block;
  }
  .life {
    stroke: var(--rt-border-strong);
    stroke-dasharray: 3 4;
  }
  .hit {
    fill: transparent;
  }
  .msg {
    cursor: default;
  }
  .msg:hover .hit {
    fill: var(--rt-bg-hover);
  }
  .msg.selected .hit {
    fill: var(--rt-bg-selected);
  }
  .arrow {
    stroke: var(--rt-accent);
    stroke-width: 1.5;
    color: var(--rt-accent);
  }
  .response .arrow {
    stroke: var(--rt-fg-muted);
    stroke-dasharray: 5 3;
  }
  .msg.selected .arrow {
    stroke-width: 2.25;
  }
  .label {
    fill: var(--rt-fg);
    font: var(--rt-font-small);
  }
  .response .label {
    fill: var(--rt-fg-muted);
  }
  .note rect {
    fill: var(--rt-warn-bg);
    stroke: var(--rt-warn-border);
  }
  .note text {
    fill: var(--rt-warn-fg);
    font: var(--rt-font-caption);
  }
  .group rect {
    fill: none;
    stroke: var(--rt-border-strong);
    stroke-dasharray: 2 3;
  }
  .group[data-kind="rect"] rect {
    fill: var(--rt-accent-subtle);
    stroke: none;
    opacity: 0.5;
  }
  .group text {
    fill: var(--rt-fg-subtle);
    font: var(--rt-font-caption);
  }
</style>
