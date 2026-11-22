<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Message history of a session (WebSocket, SSE, gRPC), wave 8. Owner: strand A; strand B
     uses it for gRPC streams. Props are the contract: `events` in order, `decode` turns a
     frame into display text (gRPC: protobuf → JSON).
     A virtual list with a fixed row height: only the rows in view exist in the DOM, so long
     histories stay smooth. While scrolled to the end, new messages keep it at the end. A row
     opens the detail below the list (pretty JSON, copy). -->
<script lang="ts">
  import type { Frame, StreamEvent } from "../commands/types";
  import { filterEvents, formatAt, prettyJson, summary } from "./kind";

  let {
    events,
    decode = (frame: Frame) => frame.data,
  }: {
    events: StreamEvent[];
    decode?: (frame: Frame) => string;
  } = $props();

  const ROW = 24;
  /** Rows rendered beyond the viewport. */
  const OVERSCAN = 8;

  let filter = $state("");
  let selected = $state<number | null>(null);
  let top = $state(0);
  let height = $state(0);
  let follow = $state(true);
  let viewport = $state<HTMLDivElement>();
  let copied = $state(false);

  const shown = $derived(filterEvents(events, filter, decode));
  const first = $derived(Math.max(0, Math.floor(top / ROW) - OVERSCAN));
  const last = $derived(Math.min(shown.length, Math.ceil((top + height) / ROW) + OVERSCAN));
  const visible = $derived(shown.slice(first, last));
  const detail = $derived(selected !== null ? events[selected] : undefined);
  const detailText = $derived(detail?.event === "message" ? decode(detail.frame) : detail ? summary(detail, decode) : "");
  const detailPretty = $derived(prettyJson(detailText));
  const messages = $derived(events.filter((e) => e.event === "message").length);

  // Stay at the end while following.
  $effect(() => {
    void shown.length;
    if (!follow || !viewport) return;
    viewport.scrollTop = viewport.scrollHeight;
    top = viewport.scrollTop;
  });

  function onscroll() {
    if (!viewport) return;
    top = viewport.scrollTop;
    follow = viewport.scrollTop + viewport.clientHeight >= viewport.scrollHeight - ROW / 2;
  }

  function onkeydown(e: KeyboardEvent) {
    if (shown.length === 0) return;
    const at = selected === null ? -1 : shown.indexOf(selected);
    let next: number | null = null;
    if (e.key === "ArrowDown") next = Math.min(shown.length - 1, at + 1);
    else if (e.key === "ArrowUp") next = Math.max(0, at - 1);
    else if (e.key === "Escape") {
      selected = null;
      return;
    }
    if (next === null) return;
    e.preventDefault();
    selected = shown[next];
    follow = false;
    if (viewport) {
      const y = next * ROW;
      if (y < viewport.scrollTop) viewport.scrollTop = y;
      else if (y + ROW > viewport.scrollTop + viewport.clientHeight) viewport.scrollTop = y + ROW - viewport.clientHeight;
    }
  }

  async function copy() {
    const text = detailPretty ?? detailText;
    try {
      await navigator.clipboard.writeText(text);
      copied = true;
      setTimeout(() => (copied = false), 1200);
    } catch {
      copied = false;
    }
  }

  function arrow(e: StreamEvent): string {
    if (e.event !== "message") return "•";
    return e.direction === "sent" ? "↑" : "↓";
  }
</script>

<div class="stream" data-testid="stream-view">
  <div class="bar">
    <input
      class="filter"
      type="search"
      data-testid="stream-filter"
      aria-label="Filter messages"
      placeholder="Filter messages"
      bind:value={filter}
    />
    <span class="count" data-testid="stream-count">
      {filter ? `${shown.length} of ${events.length}` : `${messages} message${messages === 1 ? "" : "s"}`}
    </span>
  </div>
  <div
    class="viewport"
    role="listbox"
    tabindex="0"
    aria-label="Messages"
    data-testid="stream-list"
    bind:this={viewport}
    bind:clientHeight={height}
    {onscroll}
    {onkeydown}
  >
    <div class="spacer" style:height="{shown.length * ROW}px">
      {#each visible as index, j (index)}
        {@const e = events[index]}
        <div
          class="row {e.event}"
          class:sent={e.event === "message" && e.direction === "sent"}
          class:selected={selected === index}
          role="option"
          tabindex="-1"
          aria-selected={selected === index}
          data-testid="stream-row"
          data-event={e.event}
          data-direction={e.event === "message" ? e.direction : undefined}
          style:top="{(first + j) * ROW}px"
          onclick={() => {
            selected = selected === index ? null : index;
          }}
          onkeydown={() => {}}
        >
          <span class="dir" aria-label={e.event === "message" ? e.direction : e.event}>{arrow(e)}</span>
          <span class="time">{formatAt(e.at_ms)}</span>
          {#if e.event === "message" && (e.name || e.id)}
            <span class="name" title="event {e.name ?? ''} id {e.id ?? ''}">{e.name ?? ""}{e.id ? `#${e.id}` : ""}</span>
          {/if}
          <span class="text">{summary(e, decode)}</span>
        </div>
      {/each}
    </div>
    {#if events.length === 0}
      <p class="empty">No messages yet.</p>
    {/if}
  </div>
  {#if detail}
    <div class="detail" data-testid="stream-detail">
      <div class="detail-bar">
        <span>
          {detail.event === "message" ? (detail.direction === "sent" ? "Sent" : "Received") : detail.event}
          at {formatAt(detail.at_ms)}
          {#if detail.event === "message" && detail.frame.type === "binary"}(binary, base64){/if}
        </span>
        <span class="spacer-x"></span>
        <button class="button" data-testid="stream-copy" onclick={copy}>{copied ? "Copied" : "Copy"}</button>
        <button class="button" aria-label="Close message" onclick={() => (selected = null)}>×</button>
      </div>
      <pre data-testid="stream-detail-text">{detailPretty ?? detailText}</pre>
    </div>
  {/if}
</div>

<style>
  .stream {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
    gap: 4px;
  }
  .bar,
  .detail-bar {
    display: flex;
    align-items: center;
    gap: var(--rt-gap);
  }
  .filter {
    flex: 1;
    max-width: 320px;
    font: inherit;
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    padding: 2px 6px;
  }
  .count {
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
  .viewport {
    position: relative;
    flex: 1;
    min-height: 120px;
    overflow: auto;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    font-family: var(--rt-font-mono, monospace);
    font-size: var(--rt-size-small);
  }
  .viewport:focus-visible {
    outline: 2px solid var(--rt-focus);
    outline-offset: -2px;
  }
  .spacer {
    position: relative;
  }
  .row {
    position: absolute;
    left: 0;
    right: 0;
    height: 24px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 8px;
    white-space: nowrap;
    cursor: default;
    box-sizing: border-box;
  }
  .row:hover {
    background: var(--rt-bg-hover);
  }
  .row.selected {
    background: var(--rt-bg-selected);
  }
  .row.opened,
  .row.closed {
    color: var(--rt-fg-muted);
  }
  .row.error {
    color: var(--rt-danger);
  }
  .dir {
    width: 1em;
    text-align: center;
    color: var(--rt-accent);
  }
  .row.sent .dir {
    color: var(--rt-fg-muted);
  }
  .time {
    color: var(--rt-fg-muted);
  }
  .name {
    color: var(--rt-accent);
  }
  .text {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .empty {
    margin: 8px;
    color: var(--rt-fg-muted);
    font-family: var(--rt-font, inherit);
  }
  .detail {
    display: flex;
    flex-direction: column;
    gap: 4px;
    max-height: 40%;
    min-height: 80px;
  }
  .detail pre {
    margin: 0;
    flex: 1;
    overflow: auto;
    padding: 6px 8px;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg-panel);
    font-family: var(--rt-font-mono, monospace);
    font-size: var(--rt-size-small);
    white-space: pre-wrap;
    word-break: break-word;
  }
  .spacer-x {
    flex: 1;
  }
  .button {
    height: 22px;
    padding: 0 8px;
    font: inherit;
    font-size: var(--rt-size-small);
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    cursor: pointer;
  }
  .button:hover {
    background: var(--rt-bg-hover);
  }
</style>
