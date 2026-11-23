<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The numbers of a comparison (wave 14, strand H): changed, new, removed, breaking and
     tests that went from green to red. Each is a button that narrows the list to it; pressed
     again, the list shows all changes. -->
<script lang="ts">
  import type { CompareFilter, HeaderCounts } from "./rows";

  let {
    counts,
    filter,
    ran,
    onfilter,
  }: { counts: HeaderCounts; filter: CompareFilter; ran: boolean; onfilter: (f: CompareFilter) => void } = $props();

  const tiles = $derived([
    { id: "changed" as const, label: "Changed", value: counts.changed, tone: "warn", hint: "Requests edited or answering differently" },
    { id: "new" as const, label: "New", value: counts.added, tone: "ok", hint: "Requests only at the newer revision" },
    { id: "removed" as const, label: "Removed", value: counts.removed, tone: "bad", hint: "Requests only at the older revision" },
    { id: "breaking" as const, label: "Breaking", value: counts.breaking, tone: "bad", hint: "Requests a breaking change of the API contract touches" },
    {
      id: "failures" as const,
      label: "Green → red",
      value: ran ? counts.regressed : null,
      tone: "bad",
      hint: ran ? "Requests with a test that passed before and fails now" : "Send the requests to compare their tests",
    },
  ]);
</script>

<div class="tiles" data-testid="compare-counts" role="group" aria-label="What differs">
  {#each tiles as t (t.id)}
    <button
      type="button"
      class="tile {t.tone}"
      class:zero={!t.value}
      aria-pressed={filter === t.id}
      title={t.hint}
      data-testid="compare-count-{t.id}"
      disabled={t.value === null}
      onclick={() => onfilter(filter === t.id ? "changes" : t.id)}
    >
      <span class="value">{t.value ?? "–"}</span>
      <span class="label">{t.label}</span>
    </button>
  {/each}
  <div class="rest" data-testid="compare-same">
    <span>{counts.same} unchanged</span>
    {#if counts.fixed > 0}<span class="fixed">{counts.fixed} {counts.fixed === 1 ? "test" : "tests"} red → green</span>{/if}
  </div>
</div>

<style>
  .tiles {
    display: flex;
    flex-wrap: wrap;
    align-items: stretch;
    gap: 8px;
  }
  .tile {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    min-width: 88px;
    padding: 6px 12px;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg);
    cursor: pointer;
    text-align: left;
  }
  .tile:hover:not(:disabled) {
    background: var(--rt-bg-hover);
  }
  .tile[aria-pressed="true"] {
    border-color: var(--rt-accent);
    background: var(--rt-bg-selected);
  }
  .tile:disabled {
    cursor: default;
  }
  .value {
    font-size: 20px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    line-height: 1.2;
  }
  .tile.warn:not(.zero) .value {
    color: var(--rt-warn);
  }
  .tile.ok:not(.zero) .value {
    color: var(--rt-ok);
  }
  .tile.bad:not(.zero) .value {
    color: var(--rt-danger);
  }
  .tile.zero .value {
    color: var(--rt-fg-muted);
  }
  .label {
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
  }
  .rest {
    display: flex;
    flex-direction: column;
    justify-content: center;
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
    padding: 0 4px;
  }
  .fixed {
    color: var(--rt-ok);
  }
</style>
