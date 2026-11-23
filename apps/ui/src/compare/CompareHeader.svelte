<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The numbers of a comparison (wave 14, strand H): changed, new, removed, breaking and
     tests that went from green to red. Each is a button that narrows the list to it; pressed
     again, the list shows all changes. -->
<script lang="ts">
  import { Card, Icon } from "../ui";
  import type { CompareFilter, HeaderCounts } from "./rows";

  let {
    counts,
    filter,
    ran,
    onfilter,
  }: { counts: HeaderCounts; filter: CompareFilter; ran: boolean; onfilter: (f: CompareFilter) => void } = $props();

  const tiles = $derived([
    { id: "changed" as const, label: "Changed", value: counts.changed, tone: "warn", icon: "edit", hint: "Requests edited or answering differently" },
    { id: "new" as const, label: "New", value: counts.added, tone: "ok", icon: "add", hint: "Requests only at the newer revision" },
    { id: "removed" as const, label: "Removed", value: counts.removed, tone: "danger", icon: "minus", hint: "Requests only at the older revision" },
    { id: "breaking" as const, label: "Breaking", value: counts.breaking, tone: "danger", icon: "warning", hint: "Requests a breaking change of the API contract touches" },
    {
      id: "failures" as const,
      label: "Green → red",
      value: ran ? counts.regressed : null,
      tone: "danger",
      icon: "error",
      hint: ran ? "Requests with a test that passed before and fails now" : "Send the requests to compare their tests",
    },
  ]);
</script>

<div class="tiles" data-testid="compare-counts" role="group" aria-label="What differs">
  {#each tiles as t (t.id)}
    <div class="tile t-{t.tone}" class:zero={!t.value} class:off={t.value === null} title={t.hint} data-testid="compare-count-{t.id}">
      <Card padding="sm" selected={filter === t.id} onclick={t.value === null ? undefined : () => onfilter(filter === t.id ? "changes" : t.id)}>
        <span class="top">
          <span class="icon"><Icon name={t.icon} size="sm" /></span>
          <span class="label">{t.label}</span>
        </span>
        <span class="value">{t.value ?? "–"}</span>
      </Card>
    </div>
  {/each}
  <div class="rest" data-testid="compare-same">
    <span>{counts.same} unchanged</span>
    {#if counts.fixed > 0}<span class="fixed"><Icon name="success" size="sm" /> {counts.fixed} {counts.fixed === 1 ? "test" : "tests"} red → green</span>{/if}
  </div>
</div>

<style>
  .tiles {
    display: grid;
    grid-template-columns: repeat(5, minmax(96px, 1fr)) auto;
    gap: var(--rt-space-4);
  }
  @container (max-width: 720px) {
    .tiles {
      grid-template-columns: repeat(5, minmax(0, 1fr));
      gap: var(--rt-space-3);
    }
    .rest {
      grid-column: 1 / -1;
      flex-direction: row;
      gap: var(--rt-space-5);
    }
    .label {
      font: var(--rt-font-caption);
    }
    .tile > :global(.rt-card) {
      --pad: var(--rt-space-4);
    }
  }
  .tile {
    display: flex;
    min-width: 0;
  }
  .tile > :global(.rt-card) {
    flex: 1;
    gap: var(--rt-space-2);
  }
  .tile.off > :global(.rt-card) {
    background: var(--rt-surface-1);
    box-shadow: none;
  }
  .top {
    display: flex;
    align-items: center;
    gap: var(--rt-space-3);
    min-width: 0;
  }
  .icon {
    display: inline-grid;
    place-items: center;
    width: var(--rt-control-sm);
    height: var(--rt-control-sm);
    border-radius: var(--rt-radius-md);
    background: var(--tone-bg);
    color: var(--tone-fg);
  }
  .t-warn {
    --tone-bg: var(--rt-warn-bg);
    --tone-fg: var(--rt-warn-fg);
  }
  .t-ok {
    --tone-bg: var(--rt-ok-bg);
    --tone-fg: var(--rt-ok-fg);
  }
  .t-danger {
    --tone-bg: var(--rt-danger-bg);
    --tone-fg: var(--rt-danger-fg);
  }
  .zero {
    --tone-bg: var(--rt-surface-sunken);
    --tone-fg: var(--rt-fg-subtle);
  }
  .label {
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .value {
    font: var(--rt-font-heading);
    font-variant-numeric: tabular-nums;
    color: var(--tone-fg);
  }
  .zero .value {
    color: var(--rt-fg-subtle);
  }
  .rest {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: var(--rt-space-2);
    padding: 0 var(--rt-space-2);
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
    white-space: nowrap;
  }
  .fixed {
    display: inline-flex;
    align-items: center;
    gap: var(--rt-space-2);
    color: var(--rt-ok-fg);
  }
</style>
