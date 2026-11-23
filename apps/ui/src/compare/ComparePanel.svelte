<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Two revisions compared, graphically (wave 12, strand C; wave 14, strand H). Pick both
     sides from branches, tags and commits on their lanes (or "Compare with main" / "with
     remote"), swap them, and see at once what changed in every request's definition (the
     semantic diff, no checkout); sending the requests at both revisions adds their answers
     side by side. The numbers on top narrow the list; n and p jump from change to change.
     Opened with both revisions (from the graph, a review, the bisect result) it compares at
     once. -->
<script lang="ts">
  import { onMount } from "svelte";

  import type { CollectionDiff, GitCommit, RunComparison, SpecChange } from "../commands/types";
  import { getGitViews } from "../git/views.svelte";
  import { getApp } from "../state/app.svelte";
  import CompareHeader from "./CompareHeader.svelte";
  import CompareList from "./CompareList.svelte";
  import { shortOid } from "./model";
  import RequestDetail from "./RequestDetail.svelte";
  import RevisionPicker from "./RevisionPicker.svelte";
  import { optionsFromBranches, optionsFromRefs, quickTargets, type RevisionOption } from "./revisions";
  import { buildRows, filterRows, headerCounts, nextChange, type CompareFilter } from "./rows";

  let props: { from?: string; to?: string } = $props();

  const app = getApp();
  const views = getGitViews();

  // svelte-ignore state_referenced_locally: the props only seed the fields.
  let from = $state(props.from ?? "");
  // svelte-ignore state_referenced_locally: the props only seed the fields.
  let to = $state(props.to ?? "HEAD");
  let folder = $state("");
  let send = $state(true);

  let options = $state<RevisionOption[]>([]);
  let upstream = $state<string | undefined>(undefined);
  const quick = $derived(quickTargets(options, upstream));

  /** What the result shows (the fields may have changed since). */
  let shown = $state<{ from: string; to: string; folder: string } | null>(null);
  let loading = $state(false);
  let running = $state(false);
  let error = $state<string | null>(null);
  let runError = $state<string | null>(null);
  let diff = $state<CollectionDiff | null>(null);
  let run = $state<RunComparison | null>(null);
  let contract = $state<SpecChange[]>([]);
  let commits = $state<{ from: GitCommit | null; to: GitCommit | null }>({ from: null, to: null });
  let seq = 0;

  let filter = $state<CompareFilter>("changes");
  let query = $state("");
  let selectedPath = $state<string | null>(null);

  const inFolder = (p: string) => !shown?.folder || p.startsWith(`${shown.folder.replace(/\/+$/, "")}/`);
  const rows = $derived(
    diff || run ? buildRows(diff, run, contract).filter((r) => inFolder(r.path) || (r.oldPath !== undefined && inFolder(r.oldPath))) : [],
  );
  const counts = $derived(headerCounts(rows));
  const list = $derived(filterRows(rows, filter, query));
  const selected = $derived(selectedPath === null ? -1 : list.findIndex((r) => r.path === selectedPath));
  const current = $derived(selected >= 0 ? list[selected]! : null);

  // Keep a row selected: the first change (or row) when the selection is filtered away.
  $effect(() => {
    if (list.length > 0 && selected < 0) {
      const first = nextChange(list, -1, 1);
      selectedPath = list[first >= 0 ? first : 0]!.path;
    }
  });

  onMount(() => {
    const git = app.backend.git;
    void Promise.all([git.refs().catch(() => null), git.log(30).catch(() => [] as GitCommit[])]).then(async ([refs, log]) => {
      if (refs && refs.length > 0) {
        options = optionsFromRefs(refs, log);
        upstream = refs.find((r) => r.kind === "local" && r.current)?.upstream;
      } else {
        options = optionsFromBranches(await git.branches().catch(() => []), log);
      }
      if (!from) from = quickTargets(options, upstream).main ?? "HEAD~1";
    });
    // Opened with both revisions (the graph, a review, the bisect result): compare at once.
    if (props.from && props.to) void start();
  });

  const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

  async function start() {
    const [f, t, d] = [from.trim(), to.trim(), folder.trim()];
    if (!f || !t || loading) return;
    const mine = ++seq;
    loading = true;
    error = null;
    runError = null;
    run = null;
    contract = [];
    try {
      const [between, cf, ct] = await Promise.all([
        app.backend.diffs.between(f, t),
        app.backend.git.commits(null, f, 1).catch(() => []),
        app.backend.git.commits(null, t, 1).catch(() => []),
      ]);
      if (mine !== seq) return;
      diff = between;
      commits = { from: cf[0] ?? null, to: ct[0] ?? null };
      shown = { from: f, to: t, folder: d };
      selectedPath = null;
    } catch (e) {
      if (mine === seq) {
        error = message(e);
        diff = null;
        shown = null;
      }
      return;
    } finally {
      if (mine === seq) loading = false;
    }
    void app.backend.contract
      .changes(f, t)
      .then((c) => {
        if (mine === seq) contract = c.changes;
      })
      .catch(() => {});
    if (send) await sendBoth();
  }

  async function sendBoth() {
    if (!shown || running) return;
    const mine = seq;
    running = true;
    runError = null;
    try {
      const r = await app.backend.compare.compare(
        { from: shown.from, to: shown.to, folder: shown.folder || null },
        { environment: app.environment, vars: [] },
      );
      if (mine === seq) run = r;
    } catch (e) {
      if (mine === seq) runError = message(e);
    } finally {
      running = false;
    }
  }

  function compareWith(base: string) {
    from = base;
    to = quick.current;
    void start();
  }

  function swap() {
    [from, to] = [to, from];
    if (shown) void start();
  }

  function select(i: number) {
    const r = list[i];
    if (r) selectedPath = r.path;
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.defaultPrevented || e.metaKey || e.ctrlKey || e.altKey) return;
    const t = e.target as HTMLElement | null;
    if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable)) return;
    if (e.key !== "n" && e.key !== "p") return;
    const i = nextChange(list, selected, e.key === "n" ? 1 : -1);
    if (i >= 0) {
      e.preventDefault();
      select(i);
    }
  }

  const FILTERS: { id: CompareFilter; label: string }[] = [
    { id: "changes", label: "Changes" },
    { id: "failures", label: "Failures" },
    { id: "all", label: "All" },
  ];

  const emptyText = $derived(
    rows.length === 0
      ? "No requests to compare."
      : filter === "failures"
        ? run
          ? "Nothing fails that passed before."
          : "Send the requests to see failures."
        : filter === "changes"
          ? "Every request is the same at both revisions."
          : query
            ? "No request matches."
            : "Nothing in this group.",
  );
</script>

<svelte:window {onkeydown} />

<section class="compare" data-testid="compare-panel" aria-label="Compare revisions">
  <form
    class="bar"
    onsubmit={(e) => {
      e.preventDefault();
      void start();
    }}
  >
    <div class="revs">
      <RevisionPicker label="Older (from)" bind:value={from} {options} testid="compare-from" disabled={loading} />
      <button type="button" class="swap" data-testid="compare-swap" aria-label="Swap the revisions" title="Swap the revisions" onclick={swap} disabled={loading}>⇄</button>
      <RevisionPicker label="Newer (to)" bind:value={to} {options} testid="compare-to" disabled={loading} />
    </div>
    <label class="folder">
      <span class="label">Folder</span>
      <input data-testid="compare-folder" bind:value={folder} placeholder="whole collection" disabled={loading} spellcheck="false" />
    </label>
    <div class="go">
      <label class="check" title="Runs every request at both revisions with today's environment">
        <input type="checkbox" data-testid="compare-send" bind:checked={send} /> Send requests
      </label>
      {#if running}
        <button type="button" class="button" data-testid="compare-cancel" onclick={() => void app.backend.compare.cancel()}>Cancel</button>
      {/if}
      <button type="submit" class="button primary" data-testid="compare-start" disabled={loading || !from.trim() || !to.trim()}>
        {loading ? "Comparing…" : "Compare"}
      </button>
    </div>
  </form>
  <div class="quick">
    {#if quick.main}
      <button type="button" class="chip" data-testid="compare-with-main" onclick={() => compareWith(quick.main!)} disabled={loading}>
        Compare {quick.current} with {quick.main}
      </button>
    {/if}
    {#if quick.remote}
      <button type="button" class="chip" data-testid="compare-with-remote" onclick={() => compareWith(quick.remote!)} disabled={loading}>
        Compare with {quick.remote}
      </button>
    {/if}
    <span class="hint">
      Nothing is checked out. Requests run with {app.environment ? `today's environment "${app.environment}"` : "no environment"}, its secrets, cookies and plugins.
    </span>
  </div>

  {#if error}
    <p class="error" role="alert" data-testid="compare-error">{error}</p>
  {/if}

  {#if !shown && !error && !loading}
    <div class="intro" data-testid="compare-intro">
      <strong>Pick two revisions</strong>
      <p>Choose a branch, tag or commit on each side, or start with one of the shortcuts above. You see every request's changes side by side, and with "Send requests" their answers too.</p>
    </div>
  {/if}

  {#if shown}
    <div class="result" data-testid="compare-result">
      <div class="revline" data-testid="compare-summary">
        <span class="rev"><span class="name">{shown.from}</span>{#if commits.from}<span class="oid">{shortOid(commits.from.oid)}</span><span class="msg">{commits.from.summary}</span>{/if}</span>
        <span class="arrow" aria-hidden="true">→</span>
        <span class="rev"><span class="name">{shown.to}</span>{#if commits.to}<span class="oid">{shortOid(commits.to.oid)}</span><span class="msg">{commits.to.summary}</span>{/if}</span>
        {#if running}<span class="status" data-testid="compare-running">Sending the requests at both revisions…</span>{/if}
        {#if runError}<span class="status bad" role="alert" data-testid="compare-run-error">Answers not compared: {runError}</span>{/if}
      </div>

      <CompareHeader {counts} {filter} ran={run !== null} onfilter={(f) => (filter = f)} />

      <div class="split">
        <div class="side">
          <div class="filters">
            <div class="seg" role="radiogroup" aria-label="Show">
              {#each FILTERS as f (f.id)}
                <button type="button" role="radio" aria-checked={filter === f.id} data-testid="compare-filter-{f.id}" onclick={() => (filter = f.id)}>{f.label}</button>
              {/each}
            </div>
            <input class="search" type="search" placeholder="Filter requests" aria-label="Filter requests" data-testid="compare-search" bind:value={query} />
          </div>
          {#if list.length === 0}
            <p class="empty" data-testid="compare-empty">{emptyText}</p>
          {:else}
            <CompareList rows={list} {selected} onselect={select} />
          {/if}
          <p class="keys"><kbd>n</kbd> <kbd>p</kbd> next and previous change · {list.length} of {rows.length}</p>
        </div>
        <div class="main">
          {#if current}
            <RequestDetail
              row={current}
              ran={run !== null}
              {running}
              onrun={() => void sendBoth()}
              onopen={(p) => {
                views.close();
                void app.select(p);
              }}
              onhistory={(p) => views.open({ kind: "timeline", path: p })}
            />
          {:else}
            <p class="empty">{rows.length === 0 ? "Nothing differs between these revisions." : "Pick a request on the left."}</p>
          {/if}
        </div>
      </div>
    </div>
  {/if}
</section>

<style>
  .compare {
    container-type: inline-size;
    display: flex;
    flex-direction: column;
    gap: 10px;
    height: 100%;
    min-height: 0;
    box-sizing: border-box;
    padding: 12px 16px 0;
  }
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: 10px 16px;
  }
  .revs {
    display: flex;
    align-items: flex-end;
    gap: 6px;
    flex: 1 1 420px;
    min-width: 0;
  }
  .revs > :global(.picker) {
    flex: 1 1 0;
  }
  .swap {
    height: 30px;
    width: 30px;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg);
    cursor: pointer;
  }
  .folder {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 0 1 160px;
  }
  .label {
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
  }
  .folder input,
  .search {
    height: 30px;
    box-sizing: border-box;
    padding: 0 8px;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg);
    min-width: 0;
  }
  .folder input {
    font-family: var(--rt-font-mono);
  }
  .go {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-left: auto;
  }
  .check {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    white-space: nowrap;
  }
  .button {
    height: 30px;
    padding: 0 12px;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg);
    cursor: pointer;
  }
  .button.primary:not(:disabled) {
    background: var(--rt-accent);
    border-color: var(--rt-accent);
    color: var(--rt-accent-fg);
  }
  .button:disabled,
  .chip:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .quick {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 8px;
  }
  .chip {
    height: 24px;
    padding: 0 10px;
    border: 1px solid var(--rt-border);
    border-radius: 12px;
    background: var(--rt-bg-panel);
    cursor: pointer;
    font-size: var(--rt-size-small);
  }
  .hint {
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
  }
  .error {
    margin: 0;
    color: var(--rt-danger);
  }
  .intro {
    padding: 24px;
    border: 1px dashed var(--rt-border);
    border-radius: var(--rt-radius);
    color: var(--rt-fg-muted);
  }
  .intro p {
    margin: 4px 0 0;
  }
  .intro strong {
    color: var(--rt-fg);
  }
  .result {
    display: flex;
    flex-direction: column;
    gap: 10px;
    flex: 1;
    min-height: 0;
  }
  .revline {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 4px 10px;
    min-width: 0;
  }
  .rev {
    display: inline-flex;
    align-items: baseline;
    gap: 6px;
    min-width: 0;
  }
  .rev .name {
    font-weight: 600;
  }
  .oid {
    font-family: var(--rt-font-mono);
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
  }
  .msg {
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 28ch;
  }
  .arrow {
    color: var(--rt-fg-muted);
  }
  .status {
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
  }
  .status.bad {
    color: var(--rt-danger);
  }
  .split {
    display: grid;
    grid-template-columns: minmax(240px, 340px) minmax(0, 1fr);
    flex: 1;
    min-height: 0;
    border-top: 1px solid var(--rt-border);
    margin: 0 -16px;
  }
  @container (max-width: 720px) {
    .split {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: minmax(160px, 40%) minmax(0, 1fr);
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
    background: var(--rt-bg-panel);
  }
  .filters {
    display: flex;
    gap: 6px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--rt-border);
  }
  .search {
    flex: 1;
    height: 26px;
  }
  .seg {
    display: inline-flex;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    overflow: hidden;
    flex: none;
  }
  .seg button {
    border: 0;
    padding: 0 8px;
    height: 24px;
    background: var(--rt-bg);
    cursor: pointer;
    font-size: var(--rt-size-small);
  }
  .seg button + button {
    border-left: 1px solid var(--rt-border);
  }
  .seg button[aria-checked="true"] {
    background: var(--rt-bg-selected);
    font-weight: 600;
  }
  .side > :global(.viewport) {
    flex: 1;
  }
  .empty {
    margin: 0;
    padding: 16px 12px;
    color: var(--rt-fg-muted);
  }
  .side .empty {
    flex: 1;
  }
  .keys {
    margin: 0;
    padding: 4px 12px;
    border-top: 1px solid var(--rt-border);
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
  }
  .main {
    overflow: auto;
    min-height: 0;
    min-width: 0;
  }
</style>
