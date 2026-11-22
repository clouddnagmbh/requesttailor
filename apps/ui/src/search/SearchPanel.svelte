<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Sidebar view "Search": full-text search over the open collection (`backend.search`),
     unsaved changes included. Owner: wave 4, strand E. `focus()` is called when the view is
     opened from the palette.
     Keyboard (focus stays in the field, combobox pattern): ArrowDown/ArrowUp move through the
     hits, Enter opens the request of the active hit in a tab, Escape clears the query. Hits
     are grouped by request; the matches in each snippet are highlighted. -->
<script lang="ts">
  import { tick } from "svelte";

  import type { SearchHit } from "../commands/types";
  import MethodBadge from "../shell/MethodBadge.svelte";
  import { getApp } from "../state/app.svelte";
  import { FIELD_LABELS, groupHits, segments } from "./highlight";

  const LIMIT = 200;
  const DEBOUNCE_MS = 120;
  const LIST_ID = "rt-search-results";
  const hitId = (i: number) => `rt-search-hit-${i}`;

  const app = getApp();
  let input = $state<HTMLInputElement>();
  let listEl = $state<HTMLElement>();
  let value = $state("");
  let query = $state("");
  let hits = $state<SearchHit[]>([]);
  let searched = $state(false);
  let error = $state<string | null>(null);
  let active = $state(-1);
  const groups = $derived(groupHits(hits));
  /** Hits in display order (group by group): what the arrow keys walk through. */
  const shown = $derived(groups.flatMap((g) => g.hits.map((h) => h.hit)));

  export function focus(): void {
    input?.focus();
    input?.select();
  }

  $effect(() => {
    const v = value;
    const t = setTimeout(() => (query = v), DEBOUNCE_MS);
    return () => clearTimeout(t);
  });

  // Search again when the query changes and when the collection changes (edits update the
  // tree, opening and reloading bump loadCount). Late answers of older searches are dropped.
  let generation = 0;
  $effect(() => {
    void app.tree;
    void app.loadCount;
    const q = query.trim();
    const mine = ++generation;
    if (!q || !app.collection) {
      hits = [];
      searched = false;
      active = -1;
      return;
    }
    app.backend.search(q, LIMIT).then(
      (result) => {
        if (mine !== generation) return;
        hits = result;
        searched = true;
        error = null;
        active = result.length > 0 ? 0 : -1;
      },
      (e: unknown) => {
        if (mine !== generation) return;
        error = e instanceof Error ? e.message : String(e);
      },
    );
  });

  async function move(to: number) {
    if (shown.length === 0) return;
    active = Math.max(0, Math.min(to, shown.length - 1));
    await tick();
    listEl?.querySelector(`#${hitId(active)}`)?.scrollIntoView({ block: "nearest" });
  }

  function open(i: number) {
    const hit = shown[i];
    if (!hit) return;
    active = i;
    void app.select(hit.path);
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    let handled = true;
    switch (e.key) {
      case "ArrowDown":
        move(active + 1);
        break;
      case "ArrowUp":
        move(active - 1);
        break;
      case "PageDown":
        move(active + 10);
        break;
      case "PageUp":
        move(active - 10);
        break;
      case "Enter":
        open(active);
        break;
      case "Escape":
        if (!value) {
          handled = false;
          break;
        }
        value = "";
        query = "";
        break;
      default:
        handled = false;
    }
    if (handled) {
      e.preventDefault();
      e.stopPropagation();
    }
  }

  const summary = $derived(
    !searched
      ? ""
      : hits.length === 0
        ? "No matches."
        : `${hits.length}${hits.length >= LIMIT ? "+" : ""} match${hits.length === 1 ? "" : "es"} in ${groups.length} request${groups.length === 1 ? "" : "s"}`,
  );
</script>

<section class="search" data-testid="search-panel" aria-label="Search">
  <div class="head"><span class="title">Search</span></div>
  <div class="field">
    <input
      bind:this={input}
      bind:value
      type="search"
      role="combobox"
      data-testid="search-input"
      aria-label="Search in collection"
      aria-autocomplete="list"
      aria-expanded={hits.length > 0}
      aria-controls={LIST_ID}
      aria-activedescendant={active >= 0 ? hitId(active) : undefined}
      placeholder="Names, URLs, headers, bodies…"
      autocomplete="off"
      spellcheck="false"
      {onkeydown}
    />
  </div>
  <p class="summary" role="status" data-testid="search-summary">{error ?? summary}</p>
  <div class="results" id={LIST_ID} role="listbox" aria-label="Search results" bind:this={listEl}>
    {#each groups as group (group.path)}
      <div class="group" role="group" aria-label={group.name} data-testid="search-group" data-path={group.path}>
        <div class="request" aria-hidden="true">
          <MethodBadge method={group.method} />
          <span class="name">{group.name}</span>
        </div>
        {#each group.hits as { hit, index } (index)}
          <!-- Focus stays in the search field (aria-activedescendant); the mouse may click. -->
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div
            class="hit"
            class:active={index === active}
            id={hitId(index)}
            role="option"
            tabindex="-1"
            aria-selected={index === active}
            data-testid="search-hit"
            data-field={hit.field}
            onclick={() => open(index)}
            onmousemove={() => (active = index)}
          >
            <span class="field-label">{FIELD_LABELS[hit.field]}</span>
            <span class="snippet"
              >{#each segments(hit.snippet, hit.ranges) as seg, k (k)}{#if seg.mark}<mark>{seg.text}</mark
                  >{:else}{seg.text}{/if}{/each}</span
            >
          </div>
        {/each}
      </div>
    {/each}
  </div>
</section>

<style>
  .search {
    display: flex;
    flex-direction: column;
    min-height: 0;
    height: 100%;
    background: var(--rt-bg-panel);
  }
  .head {
    padding: 6px var(--rt-gap) 6px 12px;
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .field {
    padding: 0 var(--rt-gap) 4px;
  }
  .field input {
    width: 100%;
    box-sizing: border-box;
    font: inherit;
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    padding: 3px 6px;
  }
  .field input:focus-visible {
    outline: 2px solid var(--rt-focus);
    outline-offset: -1px;
  }
  .summary {
    min-height: 1.4em;
    margin: 0;
    padding: 0 12px 4px;
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
  .results {
    flex: 1;
    overflow: auto;
    padding-bottom: var(--rt-gap);
  }
  .group + .group {
    margin-top: 4px;
  }
  .request {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 2px var(--rt-gap) 2px 12px;
    white-space: nowrap;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    font-weight: 600;
  }
  .hit {
    display: flex;
    gap: 6px;
    align-items: baseline;
    padding: 2px var(--rt-gap) 2px 24px;
    cursor: default;
    white-space: nowrap;
    font-size: var(--rt-size-small);
  }
  .hit:hover {
    background: var(--rt-bg-hover);
  }
  .hit.active {
    background: var(--rt-bg-selected);
    box-shadow: inset 2px 0 0 var(--rt-focus);
  }
  .field-label {
    flex: none;
    min-width: 4.5em;
    color: var(--rt-fg-muted);
  }
  .snippet {
    overflow: hidden;
    text-overflow: ellipsis;
    font-family: var(--rt-font-mono);
  }
  mark {
    background: none;
    color: var(--rt-accent);
    font-weight: 700;
    text-decoration: underline;
    text-underline-offset: 2px;
  }
</style>
