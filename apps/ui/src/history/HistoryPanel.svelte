<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Sidebar view "History": this device's sends from the open collection, searchable.
     Owner: wave 4, strand E. Re-reads when `app.sendCount` or `app.loadCount` changes.
     Keyboard: the filter field, then the list (one tab stop, arrows, Home/End, Enter opens
     the request in a tab). "Clear history" asks inside the panel. -->
<script lang="ts">
  import { tick } from "svelte";

  import type { HistoryEntry } from "../commands/types";
  import { describeError, formatDuration } from "../response/format";
  import MethodBadge from "../shell/MethodBadge.svelte";
  import { getApp } from "../state/app.svelte";
  import { entryClass, statusLabel, timeAgo } from "./format";

  /** Entries shown at most. */
  const LIMIT = 200;
  const DEBOUNCE_MS = 150;

  const app = getApp();
  let filter = $state("");
  let query = $state("");
  let entries = $state<HistoryEntry[]>([]);
  let loaded = $state(false);
  let error = $state<string | null>(null);
  let focusIndex = $state(0);
  let confirming = $state(false);
  let now = $state(Date.now());
  let listEl = $state<HTMLElement>();
  let clearButton = $state<HTMLButtonElement>();

  // Debounced filter.
  $effect(() => {
    const value = filter;
    const t = setTimeout(() => (query = value), DEBOUNCE_MS);
    return () => clearTimeout(t);
  });

  // Re-read after every send, (re)load and filter change. Answers that arrive out of order
  // are dropped.
  let generation = 0;
  $effect(() => {
    void app.sendCount;
    void app.loadCount;
    const collection = app.collection?.id;
    const q = query;
    const mine = ++generation;
    if (!collection) {
      entries = [];
      return;
    }
    app.backend.history.list(q, LIMIT).then(
      (list) => {
        if (mine !== generation) return;
        entries = list;
        loaded = true;
        error = null;
        now = Date.now();
        focusIndex = Math.min(focusIndex, Math.max(0, list.length - 1));
      },
      (e: unknown) => {
        if (mine !== generation) return;
        error = e instanceof Error ? e.message : String(e);
      },
    );
  });

  // "2 min ago" stays current.
  $effect(() => {
    const t = setInterval(() => (now = Date.now()), 30_000);
    return () => clearInterval(t);
  });

  async function focusRow(i: number) {
    if (entries.length === 0) return;
    focusIndex = Math.max(0, Math.min(i, entries.length - 1));
    await tick();
    listEl?.querySelector<HTMLElement>(`[data-index="${focusIndex}"]`)?.focus();
  }

  function open(entry: HistoryEntry, i: number) {
    focusIndex = i;
    void app.select(entry.request_path);
  }

  function onRowKeydown(e: KeyboardEvent, entry: HistoryEntry, i: number) {
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    let handled = true;
    switch (e.key) {
      case "ArrowDown":
        focusRow(i + 1);
        break;
      case "ArrowUp":
        focusRow(i - 1);
        break;
      case "Home":
        focusRow(0);
        break;
      case "End":
        focusRow(entries.length - 1);
        break;
      case "Enter":
      case " ":
        open(entry, i);
        break;
      default:
        handled = false;
    }
    if (handled) e.preventDefault();
  }

  function onFilterKeydown(e: KeyboardEvent) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      focusRow(0);
    } else if (e.key === "Escape" && filter) {
      e.preventDefault();
      e.stopPropagation();
      filter = "";
    }
  }

  async function clearHistory() {
    confirming = false;
    try {
      await app.backend.history.clear();
      entries = [];
      error = null;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
    await tick();
    clearButton?.focus();
  }

  async function cancelClear() {
    confirming = false;
    await tick();
    clearButton?.focus();
  }

  function onConfirmKeydown(e: KeyboardEvent) {
    if (e.key !== "Escape") return;
    e.preventDefault();
    e.stopPropagation();
    cancelClear();
  }

  const autofocus = (el: HTMLElement) => {
    el.focus();
  };

  function detail(entry: HistoryEntry): string {
    const parts = [new Date(entry.at).toLocaleString()];
    if (entry.environment) parts.push(`Environment: ${entry.environment}`);
    if (entry.url) parts.push(entry.url);
    if (entry.error) parts.push(describeError(entry.error));
    return parts.join("\n");
  }
</script>

<section class="history" data-testid="history-panel" aria-label="History">
  <div class="head">
    <span class="title">History</span>
    {#if entries.length > 0 || query}
      <button
        class="link"
        data-testid="history-clear"
        bind:this={clearButton}
        disabled={confirming}
        onclick={() => (confirming = true)}>Clear history</button
      >
    {/if}
  </div>
  {#if confirming}
    <div class="confirm" role="group" aria-label="Clear history" data-testid="history-confirm">
      <p>Clear the history of this collection on this device?</p>
      <div class="actions">
        <button
          class="button danger"
          data-testid="history-clear-confirm"
          onclick={clearHistory}
          onkeydown={onConfirmKeydown}
          {@attach autofocus}>Clear</button
        >
        <button
          class="button"
          data-testid="history-clear-cancel"
          onclick={cancelClear}
          onkeydown={onConfirmKeydown}>Cancel</button
        >
      </div>
    </div>
  {/if}
  <div class="filter">
    <input
      type="search"
      data-testid="history-filter"
      aria-label="Filter history"
      placeholder="Filter: name, URL, 404, 5xx…"
      bind:value={filter}
      onkeydown={onFilterKeydown}
    />
  </div>
  {#if error}
    <p class="message error" role="alert">{error}</p>
  {:else if loaded && entries.length === 0}
    <p class="message" data-testid="history-empty">
      {query ? "No sends match the filter." : "Nothing sent from this collection yet."}
    </p>
  {/if}
  <div class="list" role="listbox" aria-label="Sent requests" bind:this={listEl} data-testid="history-list">
    {#each entries as entry, i (entry.id)}
      {@const cls = entryClass(entry)}
      <div
        class="entry"
        class:active={entry.request_path === app.active}
        role="option"
        aria-selected={i === focusIndex}
        tabindex={i === focusIndex ? 0 : -1}
        data-index={i}
        data-testid="history-entry"
        data-path={entry.request_path}
        title={detail(entry)}
        onclick={() => open(entry, i)}
        onkeydown={(e) => onRowKeydown(e, entry, i)}
        onfocus={() => (focusIndex = i)}
      >
        <div class="line">
          <MethodBadge method={entry.method} />
          <span class="name">{entry.name}</span>
          <span class="status {cls}" data-testid="history-status" data-class={cls}>{statusLabel(entry)}</span>
        </div>
        <div class="line meta">
          <span class="url">{entry.url ?? (entry.error ? describeError(entry.error) : "")}</span>
          {#if entry.duration_ms != null}<span data-testid="history-duration">{formatDuration(entry.duration_ms)}</span>{/if}
          <time datetime={new Date(entry.at).toISOString()} data-testid="history-time">{timeAgo(entry.at, now)}</time>
        </div>
      </div>
    {/each}
  </div>
</section>

<style>
  .history {
    display: flex;
    flex-direction: column;
    min-height: 0;
    height: 100%;
    background: var(--rt-bg-panel);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px var(--rt-gap) 6px 12px;
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .link {
    border: 0;
    background: none;
    color: var(--rt-fg-muted);
    font: inherit;
    text-transform: none;
    letter-spacing: normal;
    cursor: pointer;
    padding: 2px 4px;
    border-radius: var(--rt-radius);
  }
  .link:hover:not(:disabled) {
    color: var(--rt-fg);
    background: var(--rt-bg-hover);
  }
  .link:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .confirm {
    margin: 0 var(--rt-gap) var(--rt-gap);
    padding: var(--rt-gap);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg);
  }
  .confirm p {
    margin: 0 0 var(--rt-gap);
  }
  .actions {
    display: flex;
    gap: var(--rt-gap);
  }
  .button {
    height: 24px;
    padding: 0 10px;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg);
    color: var(--rt-fg);
    font: inherit;
    cursor: pointer;
  }
  .button:hover {
    background: var(--rt-bg-hover);
  }
  .button.danger {
    background: var(--rt-danger);
    border-color: var(--rt-danger);
    color: var(--rt-accent-fg);
  }
  .filter {
    padding: 0 var(--rt-gap) 6px;
  }
  .filter input {
    width: 100%;
    box-sizing: border-box;
    font: inherit;
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    padding: 3px 6px;
  }
  .filter input:focus-visible {
    outline: 2px solid var(--rt-focus);
    outline-offset: -1px;
  }
  .list {
    flex: 1;
    overflow: auto;
    padding-bottom: var(--rt-gap);
  }
  .entry {
    padding: 4px var(--rt-gap) 4px 12px;
    cursor: default;
    user-select: none;
  }
  .entry:hover {
    background: var(--rt-bg-hover);
  }
  .entry.active {
    background: var(--rt-bg-selected);
  }
  .entry:focus-visible {
    outline: 2px solid var(--rt-focus);
    outline-offset: -2px;
  }
  .line {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    white-space: nowrap;
  }
  .name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .meta {
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
  .url {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    font-family: var(--rt-font-mono);
  }
  .status {
    font: 600 var(--rt-size-small) / 1 var(--rt-font-mono);
  }
  .ok {
    color: var(--rt-ok);
  }
  .redirect {
    color: var(--rt-accent);
  }
  .client-error {
    color: var(--rt-warn);
  }
  .server-error,
  .error {
    color: var(--rt-danger);
  }
  .info {
    color: var(--rt-fg-muted);
  }
  .message {
    margin: 0;
    padding: 0 12px;
    color: var(--rt-fg-muted);
  }
  .message.error {
    color: var(--rt-danger);
  }
</style>
