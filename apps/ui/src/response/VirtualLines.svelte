<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Virtual list of lines with fixed row height. Only the rows in view exist in the DOM and
     only their chunks are fetched (LineCache, bounded). Vertical scrolling is our own model:
     1.6 million lines (100 MB) at 18 px are ~30 million px, more than browsers can lay out,
     so the scrollbar maps the whole range onto its track instead of using native scrolling.
     The source is fixed for the component's lifetime; wrap it in {#key} to replace it. -->
<script lang="ts">
  import {
    clampTop,
    LineCache,
    maxTop,
    thumb,
    topForThumb,
    visibleRange,
    type LineSource,
  } from "./lines";

  let { source, label }: { source: LineSource; label: string } = $props();
  const id = $props.id();

  const ROW = 18;
  /** Lines fetched beyond the viewport, so short scrolls find their rows cached. */
  const MARGIN = 128;

  let tick = $state(0);
  let loadError = $state<string | null>(null);
  // The source is fixed for the lifetime of the component (see above).
  // svelte-ignore state_referenced_locally
  const cache = new LineCache(source, () => tick++);
  $effect(() => () => cache.dispose());

  let top = $state(0);
  let viewportHeight = $state(0);
  let trackHeight = $state(0);
  let viewport: HTMLDivElement;
  let track: HTMLDivElement;

  const model = $derived({ count: source.count, rowHeight: ROW, viewport: viewportHeight });
  const range = $derived(visibleRange(model, top));
  const bar = $derived(thumb(model, trackHeight, top));
  const scrollable = $derived(maxTop(model) > 0);
  const gutter = $derived(String(Math.max(1, source.count)).length);
  const offset = $derived(Math.round(top - range.first * ROW));

  const rows = $derived.by(() => {
    void tick;
    const out: { i: number; text: string; cut: boolean; loaded: boolean }[] = [];
    for (let i = range.first; i < range.first + range.rows; i++) {
      const line = cache.get(i);
      out.push({ i, text: line?.text ?? "", cut: line?.cut ?? false, loaded: line !== undefined });
    }
    return out;
  });

  $effect(() => {
    const { first, rows: n } = range;
    if (source.count === 0) return;
    cache.ensure(first - MARGIN, first + n + MARGIN).catch((e: unknown) => {
      loadError = e instanceof Error ? e.message : String(e);
    });
  });

  // The viewport can grow or shrink: keep the position valid.
  $effect(() => {
    const clamped = clampTop(model, top);
    if (clamped !== top) top = clamped;
  });

  function scrollTo(next: number) {
    top = clampTop(model, next);
  }

  function onwheel(e: WheelEvent) {
    // Horizontal gestures scroll the viewport natively (long lines).
    if (Math.abs(e.deltaX) > Math.abs(e.deltaY)) return;
    e.preventDefault();
    const unit = e.deltaMode === 1 ? ROW : e.deltaMode === 2 ? viewportHeight : 1;
    scrollTo(top + e.deltaY * unit);
  }

  $effect(() => {
    // Not passive: the wheel must not scroll the page around the list.
    viewport.addEventListener("wheel", onwheel, { passive: false });
    track.addEventListener("wheel", onwheel, { passive: false });
    return () => {
      viewport.removeEventListener("wheel", onwheel);
      track.removeEventListener("wheel", onwheel);
    };
  });

  function onkeydown(e: KeyboardEvent) {
    const page = Math.max(ROW, viewportHeight - ROW);
    const moves: Record<string, number> = {
      ArrowDown: top + ROW,
      ArrowUp: top - ROW,
      PageDown: top + page,
      PageUp: top - page,
      " ": e.shiftKey ? top - page : top + page,
      Home: 0,
      End: maxTop(model),
    };
    if (!(e.key in moves)) return;
    e.preventDefault();
    scrollTo(moves[e.key]);
  }

  let drag: { pointer: number; startY: number; startOffset: number } | null = null;

  function onpointerdown(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    const rect = track.getBoundingClientRect();
    const y = e.clientY - rect.top;
    let startOffset = bar.offset;
    if (y < bar.offset || y > bar.offset + bar.size) {
      // Click on the track: centre the thumb there, then keep dragging from it.
      startOffset = y - bar.size / 2;
      scrollTo(topForThumb(model, trackHeight, startOffset));
    }
    drag = { pointer: e.pointerId, startY: e.clientY, startOffset };
    track.setPointerCapture(e.pointerId);
  }

  function onpointermove(e: PointerEvent) {
    if (!drag || e.pointerId !== drag.pointer) return;
    scrollTo(topForThumb(model, trackHeight, drag.startOffset + e.clientY - drag.startY));
  }

  function onpointerup(e: PointerEvent) {
    if (drag?.pointer !== e.pointerId) return;
    drag = null;
    if (track.hasPointerCapture(e.pointerId)) track.releasePointerCapture(e.pointerId);
  }
</script>

<div class="lines">
  <div
    class="viewport"
    role="textbox"
    aria-readonly="true"
    aria-multiline="true"
    aria-label={label}
    tabindex="0"
    id="{id}-lines"
    data-testid="response-lines"
    bind:this={viewport}
    bind:clientHeight={viewportHeight}
    {onkeydown}
  >
    <div class="rows" style:transform="translateY({-offset}px)" style:--gutter="{gutter}ch">
      {#each rows as row (row.i)}
        <div class="row" class:loading={!row.loaded} data-testid="response-line">
          <span class="ln" aria-hidden="true">{row.i + 1}</span><span class="tx">{row.text}</span
          >{#if row.cut}<span class="cut" title="Line cut at 10 000 characters">… cut</span>{/if}
        </div>
      {/each}
    </div>
    {#if loadError}<p class="error" role="alert">{loadError}</p>{/if}
  </div>
  <div
    class="track"
    class:hidden={!scrollable}
    role="scrollbar"
    aria-controls="{id}-lines"
    aria-orientation="vertical"
    aria-valuemin={0}
    aria-valuemax={Math.max(0, source.count - 1)}
    aria-valuenow={range.first}
    tabindex="-1"
    data-testid="response-scrollbar"
    bind:this={track}
    bind:clientHeight={trackHeight}
    {onpointerdown}
    {onpointermove}
    {onpointerup}
    onpointercancel={onpointerup}
  >
    <div
      class="thumb"
      data-testid="response-thumb"
      style:height="{bar.size}px"
      style:transform="translateY({bar.offset}px)"
    ></div>
  </div>
</div>

<style>
  .lines {
    position: relative;
    display: flex;
    flex: 1 1 auto;
    min-height: 0;
    font: var(--rt-size-small) / 18px var(--rt-font-mono);
    background: var(--rt-bg);
  }
  .viewport {
    position: relative;
    flex: 1 1 auto;
    min-width: 0;
    overflow-x: auto;
    overflow-y: hidden;
    outline: none;
    overscroll-behavior: contain;
  }
  .viewport:focus-visible {
    box-shadow: inset 0 0 0 1px var(--rt-accent);
  }
  /* Out of flow: the viewport's height comes from the layout, never from its rows (which
     depend on that height). Still counts for horizontal overflow. */
  .rows {
    position: absolute;
    top: 0;
    left: 0;
    width: max-content;
    min-width: 100%;
    will-change: transform;
  }
  .row {
    height: 18px;
    white-space: pre;
    overflow: hidden;
  }
  .ln {
    position: sticky;
    left: 0;
    display: inline-block;
    box-sizing: content-box;
    width: var(--gutter);
    padding: 0 8px 0 4px;
    margin-right: 8px;
    text-align: right;
    color: var(--rt-fg-muted);
    background: var(--rt-bg-panel);
    border-right: 1px solid var(--rt-border);
    user-select: none;
  }
  .loading .tx::before {
    content: "…";
    color: var(--rt-fg-muted);
  }
  .cut {
    margin-left: 8px;
    padding: 0 4px;
    border-radius: var(--rt-radius);
    color: var(--rt-warn);
    border: 1px solid var(--rt-border);
    user-select: none;
  }
  .error {
    position: absolute;
    left: 8px;
    bottom: 8px;
    margin: 0;
    color: var(--rt-danger);
    background: var(--rt-bg-panel);
  }
  .track {
    position: relative;
    flex: 0 0 12px;
    background: var(--rt-bg-panel);
    border-left: 1px solid var(--rt-border);
    touch-action: none;
    cursor: default;
  }
  .track.hidden {
    visibility: hidden;
    flex-basis: 0;
    border: 0;
  }
  .thumb {
    position: absolute;
    left: 2px;
    right: 2px;
    top: 0;
    border-radius: var(--rt-radius);
    background: var(--rt-fg-muted);
    opacity: 0.5;
  }
  .track:hover .thumb {
    opacity: 0.8;
  }
</style>
