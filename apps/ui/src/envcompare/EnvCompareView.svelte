<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Comparing environments (wave 18, strand A): the same reading requests of the collection in
     two or more environments, side by side, in the views of the revision comparison (answers
     with the time bars, headers, JSON tree and word diff). Pick the environments, optionally a
     folder, and send. Writing requests are not sent and are named. A production environment
     asks once. Nothing is stored. n and p jump from difference to difference. -->
<script lang="ts">
  import { onMount } from "svelte";

  import type { AgentPolicy, EnvComparison } from "../commands/types";
  import { filterRows, nextChange, type CompareFilter, type CompareRow } from "../compare/rows";
  import ResponseSides from "../compare/ResponseSides.svelte";
  import type { ToolProps } from "../shell/tool-host";
  import { Badge, Banner, Button, Checkbox, Chip, Dialog, DialogFooter, EmptyState, Kbd, Spinner, TextField } from "../ui";
  import EnvPicker from "./EnvPicker.svelte";
  import { canCompare, defaultSelection, pairCounts, pairTitle, productionNames, rowsOfPair, skippedLine } from "./model";
  import { presetHolder } from "./open.svelte";

  let { app }: ToolProps = $props();

  const names = $derived(app.environments.map((e) => e.name));
  let selected = $state<string[]>([]);
  let folder = $state("");
  let ignoreValues = $state(false);
  let policy = $state<AgentPolicy | null>(null);
  const production = $derived(productionNames(policy, names));

  let running = $state(false);
  let error = $state<string | null>(null);
  let result = $state<EnvComparison | null>(null);
  let shownPair = $state(0);
  let filter = $state<CompareFilter>("changes");
  let query = $state("");
  let selectedPath = $state<string | null>(null);
  /** The open "Send reads to PRODUCTION?" question. */
  let asking = $state<string[] | null>(null);
  let seq = 0;

  const pair = $derived(result?.pairs[Math.min(shownPair, (result?.pairs.length ?? 1) - 1)] ?? null);
  const rows = $derived(pair ? rowsOfPair(pair) : []);
  const counts = $derived(pair ? pairCounts(pair) : null);
  const list = $derived(filterRows(rows, filter, query));
  const selectedIndex = $derived(selectedPath === null ? -1 : list.findIndex((r) => r.path === selectedPath));
  const current = $derived<CompareRow | null>(selectedIndex >= 0 ? list[selectedIndex]! : null);

  // Keep a row selected: the first difference (or row) when the selection is filtered away.
  $effect(() => {
    if (list.length > 0 && selectedIndex < 0) {
      const first = nextChange(list, -1, 1);
      selectedPath = list[first >= 0 ? first : 0]!.path;
    }
  });

  onMount(() => {
    const preset = presetHolder.take();
    const valid = (preset?.environments ?? []).filter((n) => names.includes(n));
    selected = valid.length >= 2 ? valid : defaultSelection(names, app.environment);
    folder = preset?.folder ?? "";
    void app.backend.agents
      .policy()
      .then((p) => (policy = p))
      .catch(() => {});
  });

  const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

  function start() {
    if (!canCompare(selected) || running) return;
    const prod = productionNames(policy, selected);
    if (prod.length > 0) asking = prod;
    else void send(false);
  }

  async function send(confirmed: boolean) {
    asking = null;
    const mine = ++seq;
    running = true;
    error = null;
    try {
      const r = await app.backend.envCompare.run({
        environments: [...selected],
        folder: folder.trim() || null,
        ignore_env_values: ignoreValues,
        production_confirmed: confirmed,
      });
      if (mine !== seq) return;
      result = r;
      shownPair = 0;
      selectedPath = null;
      filter = "changes";
      query = "";
    } catch (e) {
      if (mine === seq) {
        error = message(e);
        result = null;
      }
    } finally {
      if (mine === seq) running = false;
    }
  }

  function select(i: number) {
    const r = list[i];
    if (r) selectedPath = r.path;
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.defaultPrevented || e.metaKey || e.ctrlKey || e.altKey || asking) return;
    const t = e.target as HTMLElement | null;
    if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable)) return;
    if (e.key !== "n" && e.key !== "p") return;
    const i = nextChange(list, selectedIndex, e.key === "n" ? 1 : -1);
    if (i >= 0) {
      e.preventDefault();
      select(i);
    }
  }

  const FILTERS: { id: CompareFilter; label: string }[] = [
    { id: "changes", label: "Differences" },
    { id: "all", label: "All" },
  ];
  const STATUS = { new: "A", removed: "D", changed: "M", same: "" } as const;

  function summaryOf(r: CompareRow): string {
    const c = r.response;
    if (!c || c.same) return "same answer";
    const parts: string[] = [];
    if (c.status) parts.push(`status ${c.status.old ?? "none"} → ${c.status.new ?? "none"}`);
    if (c.error) parts.push(c.error.new ? "fails in this environment" : "fails in the reference");
    if (c.headers.length > 0) parts.push(`${c.headers.length} ${c.headers.length === 1 ? "header" : "headers"}`);
    if (c.body.length > 0) parts.push(`${c.body.length} body ${c.body.length === 1 ? "value" : "values"}`);
    if (c.body_text) parts.push("body text");
    return parts.length > 0 ? parts.join(" · ") : "not sent in one environment";
  }
</script>

<svelte:window {onkeydown} />

<section class="envcompare" data-testid="envcompare-panel" aria-label="Compare environments">
  <form
    class="bar"
    onsubmit={(e) => {
      e.preventDefault();
      start();
    }}
  >
    <EnvPicker environments={names} bind:selected {production} disabled={running} />
    <div class="folder">
      <TextField label="Folder" icon="folder" mono bind:value={folder} placeholder="whole collection" disabled={running} data-testid="envcompare-folder" />
    </div>
    <div class="go">
      <span class="ignore" title="Hide differences that only come from the environments' own variables (hosts, base URLs)">
        <Checkbox label="Ignore values of environment variables" bind:checked={ignoreValues} data-testid="envcompare-ignore" />
      </span>
      <Button type="submit" variant="primary" icon="send" loading={running} data-testid="envcompare-start" disabled={!canCompare(selected)}>
        {running ? "Sending…" : "Compare"}
      </Button>
    </div>
  </form>
  <p class="note">Only reading requests are sent, one after the other in each environment. Writing requests are skipped. Secrets, cookies and tokens stay with their environment; nothing is saved.</p>

  {#if error}
    <div data-testid="envcompare-error"><Banner tone="danger" title="Could not compare">{error}</Banner></div>
  {/if}

  {#if !result && !error && !running}
    <div class="intro" data-testid="envcompare-intro">
      <EmptyState
        icon="compare"
        title={names.length < 2 ? "Needs two environments" : "Pick the environments"}
        text={names.length < 2
          ? "This collection has fewer than two environments. Add another one in the environment manager to compare them."
          : "Choose two or more environments. The first is the reference; every request that only reads is sent in each of them."}
      />
    </div>
  {/if}

  {#if running && !result}
    <div class="loading" data-testid="envcompare-running"><Spinner label="Sending" /> Sending the requests in {selected.length} environments…</div>
  {/if}

  {#if result && pair && counts}
    <div class="result" data-testid="envcompare-result">
      {#if result.pairs.length > 1}
        <div class="pairs" role="group" aria-label="Environment">
          {#each result.pairs as p, i (p.environment)}
            <Chip label="{result.reference} ↔ {p.environment}" selected={shownPair === i} data-testid="envcompare-pair-{p.environment}" onclick={() => ((shownPair = i), (selectedPath = null))} />
          {/each}
        </div>
      {/if}
      <div class="revline" data-testid="envcompare-summary" data-different={counts.different} data-total={counts.total}>
        <strong>{pairTitle(result, pair)}</strong>
        {#if counts.status > 0}<Badge tone="warn" value="{counts.status} status" />{/if}
        {#if counts.errors > 0}<Badge tone="danger" value="{counts.errors} failing" />{/if}
      </div>

      {#if result.skipped.length > 0}
        <div class="skipped" data-testid="envcompare-skipped">
          <Banner tone="info" title="{result.skipped.length} {result.skipped.length === 1 ? 'request was' : 'requests were'} not sent">
            <ul>
              {#each result.skipped as s (s.path)}<li>{skippedLine(s)}</li>{/each}
            </ul>
          </Banner>
        </div>
      {/if}

      <div class="split">
        <div class="side">
          <div class="filters">
            <div class="chips" role="group" aria-label="Show">
              {#each FILTERS as f (f.id)}
                <Chip size="sm" label={f.label} selected={filter === f.id} data-testid="envcompare-filter-{f.id}" onclick={() => (filter = f.id)} />
              {/each}
            </div>
            <TextField size="sm" icon="search" type="search" placeholder="Filter requests" aria-label="Filter requests" data-testid="envcompare-search" bind:value={query} />
          </div>
          {#if list.length === 0}
            <div class="side-empty" data-testid="envcompare-empty">
              <EmptyState
                size="sm"
                icon={rows.length === 0 ? "filter" : "success"}
                title={rows.length === 0 ? "No requests" : "No differences"}
                text={rows.length === 0 ? "No reading request was sent." : filter === "changes" ? "Every request answers the same in both environments." : "No request matches."}
              />
            </div>
          {:else}
            <ul class="rows" data-testid="envcompare-list">
              {#each list as r, i (r.path)}
                <li>
                  <button type="button" class="row" class:on={i === selectedIndex} data-testid="envcompare-row" data-path={r.path} data-same={r.state === "same"} onclick={() => select(i)}>
                    {#if r.state !== "same"}<Badge kind="status" value={STATUS[r.state]} />{/if}
                    <span class="name">{r.name}</span>
                    <span class="what">{summaryOf(r)}</span>
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
          <p class="keys"><Kbd keys="N" size="sm" /><Kbd keys="P" size="sm" /> next and previous difference <span class="of">{list.length} of {rows.length}</span></p>
        </div>
        <div class="main">
          {#if current?.response}
            <article class="detail" data-testid="envcompare-detail" data-path={current.path} aria-label={current.name}>
              <header>
                <h3>{current.name}</h3>
                <span class="mono">{current.path}</span>
                <Button size="sm" icon="external-link" data-testid="envcompare-open-request" onclick={() => void app.select(current.path)}>Open request</Button>
              </header>
              <p class="legend" data-testid="envcompare-legend">
                <span>Before: <strong>{result.reference}</strong> (reference)</span>
                <span>After: <strong>{pair.environment}</strong></span>
              </p>
              <ResponseSides response={current.response} />
            </article>
          {:else}
            <EmptyState icon="compare" title="Pick a request" text="Choose a request on the left to see both answers side by side." />
          {/if}
        </div>
      </div>
    </div>
  {/if}
</section>

{#if asking}
  <Dialog title="Send reads to PRODUCTION?" description="{asking.join(', ')} {asking.length === 1 ? 'is a production environment' : 'are production environments'}." icon="warning" size="sm" testid="envcompare-confirm" closeOnBackdrop={false} onclose={() => (asking = null)}>
    <p class="ask">Only reading requests are sent. {selected.length} environments, one request at a time.</p>
    <DialogFooter>
      <Button variant="secondary" data-testid="envcompare-confirm-cancel" onclick={() => (asking = null)}>Cancel</Button>
      <Button variant="danger" data-testid="envcompare-confirm-yes" data-autofocus onclick={() => void send(true)}>Send</Button>
    </DialogFooter>
  </Dialog>
{/if}

<style>
  .envcompare {
    container-type: inline-size;
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-5);
    flex: 1;
    min-height: 0;
    box-sizing: border-box;
    padding: var(--rt-space-6) var(--rt-space-6) 0;
  }
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--rt-space-5) var(--rt-space-6);
  }
  .folder {
    flex: 0 1 180px;
    min-width: 120px;
  }
  .go {
    display: flex;
    align-items: center;
    gap: var(--rt-space-5);
    margin-left: auto;
    min-height: var(--rt-control-md);
  }
  .ignore {
    display: inline-flex;
    white-space: nowrap;
  }
  .note {
    margin: 0;
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .intro {
    padding: var(--rt-space-7) 0;
  }
  .loading {
    display: flex;
    align-items: center;
    gap: var(--rt-space-4);
    color: var(--rt-fg-muted);
  }
  .result {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-5);
    flex: 1;
    min-height: 0;
  }
  .pairs,
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--rt-space-3);
  }
  .revline {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-3) var(--rt-space-4);
  }
  .skipped ul {
    margin: 0;
    padding-left: var(--rt-space-6);
  }
  .split {
    display: grid;
    grid-template-columns: minmax(260px, 380px) minmax(0, 1fr);
    flex: 1;
    min-height: 0;
    margin: 0 calc(-1 * var(--rt-space-6));
    border-top: 1px solid var(--rt-border);
  }
  @container (max-width: 720px) {
    .split {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: minmax(200px, 42%) minmax(0, 1fr);
    }
    .side {
      border-right: 0;
      border-bottom: 1px solid var(--rt-border);
    }
  }
  .side {
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-right: 1px solid var(--rt-border);
    background: var(--rt-surface-sunken);
  }
  .filters {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-4);
    padding: var(--rt-space-4) var(--rt-space-5);
    border-bottom: 1px solid var(--rt-border);
  }
  .side-empty {
    flex: 1;
    padding: var(--rt-space-6) var(--rt-space-5);
  }
  .rows {
    flex: 1;
    min-height: 0;
    margin: 0;
    padding: 0;
    overflow: auto;
    list-style: none;
  }
  .row {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    grid-template-areas: "badge name" "badge what";
    align-items: center;
    column-gap: var(--rt-space-4);
    width: 100%;
    padding: var(--rt-space-3) var(--rt-space-5);
    border: 0;
    border-bottom: 1px solid var(--rt-border);
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .row:not(:has(:global(.rt-badge))) {
    grid-template-columns: minmax(0, 1fr);
    grid-template-areas: "name" "what";
  }
  .row:hover {
    background: var(--rt-surface-2);
  }
  .row.on {
    background: var(--rt-accent-bg, var(--rt-surface-3));
  }
  .row :global(.rt-badge) {
    grid-area: badge;
  }
  .name {
    grid-area: name;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .what {
    grid-area: what;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .keys {
    display: flex;
    align-items: center;
    gap: var(--rt-space-2);
    margin: 0;
    padding: var(--rt-space-3) var(--rt-space-5);
    border-top: 1px solid var(--rt-border);
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .keys .of {
    margin-left: auto;
    font-variant-numeric: tabular-nums;
  }
  .main {
    overflow: auto;
    min-height: 0;
    min-width: 0;
    background: var(--rt-surface-1);
  }
  .main > :global(.rt-empty) {
    margin-top: var(--rt-space-8);
  }
  .detail {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-5);
    padding: var(--rt-space-6);
    min-width: 0;
  }
  .detail header {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-3) var(--rt-space-5);
  }
  .detail h3 {
    margin: 0;
    font: var(--rt-font-heading);
  }
  .mono {
    font-family: var(--rt-font-mono);
    color: var(--rt-fg-muted);
    font-size: var(--rt-text-sm, 0.85em);
  }
  .detail header :global(button) {
    margin-left: auto;
  }
  .legend {
    display: flex;
    gap: var(--rt-space-6);
    margin: 0;
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .ask {
    margin: 0 0 var(--rt-space-5);
  }
</style>
