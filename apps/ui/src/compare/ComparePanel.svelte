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
  import { Banner, Button, Checkbox, Chip, EmptyState, Icon, IconButton, Kbd, Spinner, TextField } from "../ui";
  import { extensionRangeActions } from "../shell/extension-actions";
  import { getExtensions } from "../shell/extension-tabs";
  import ExtensionActions from "../shell/ExtensionActions.svelte";
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
  // Wave 16: actions of extensions on the two revisions compared.
  const extensions = getExtensions();
  const rangeActions = (from: string, to: string) =>
    extensionRangeActions(extensions, { collection: app.activeCollection, from, to, source: "compare" });
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
      <span class="swap">
        <IconButton icon="compare" label="Swap the revisions" variant="secondary" data-testid="compare-swap" onclick={swap} disabled={loading} />
      </span>
      <RevisionPicker label="Newer (to)" bind:value={to} {options} testid="compare-to" disabled={loading} />
    </div>
    <div class="folder">
      <TextField label="Folder" icon="folder" mono bind:value={folder} placeholder="whole collection" disabled={loading} data-testid="compare-folder" />
    </div>
    <div class="go">
      <span class="send" title="Runs every request at both revisions with today's environment">
        <Checkbox label="Send requests" bind:checked={send} data-testid="compare-send" />
      </span>
      {#if running}
        <Button data-testid="compare-cancel" onclick={() => void app.backend.compare.cancel()}>Cancel</Button>
      {/if}
      <Button type="submit" variant="primary" icon="compare" loading={loading} data-testid="compare-start" disabled={!from.trim() || !to.trim()}>
        {loading ? "Comparing…" : "Compare"}
      </Button>
    </div>
  </form>
  <div class="quick">
    {#if quick.main}
      <Chip icon="branch" label="Compare {quick.current} with {quick.main}" data-testid="compare-with-main" onclick={() => compareWith(quick.main!)} disabled={loading} />
    {/if}
    {#if quick.remote}
      <Chip icon="sync" label="Compare with {quick.remote}" data-testid="compare-with-remote" onclick={() => compareWith(quick.remote!)} disabled={loading} />
    {/if}
    <span class="hint">
      Nothing is checked out. Requests run with {app.environment ? `today's environment "${app.environment}"` : "no environment"}, its secrets, cookies and plugins.
    </span>
  </div>

  {#if error}
    <div data-testid="compare-error"><Banner tone="danger" title="Could not compare">{error}</Banner></div>
  {/if}

  {#if !shown && !error && !loading}
    <div class="intro" data-testid="compare-intro">
      <EmptyState
        icon="compare"
        title="Pick two revisions"
        text={'Choose a branch, tag or commit on each side, or start with a shortcut above. Every changed request appears side by side, with "Send requests" its answers too.'}
      />
    </div>
  {/if}

  {#if loading && !shown}
    <div class="loading"><Spinner label="Comparing" /> Reading both revisions…</div>
  {/if}

  {#if shown}
    <div class="result" data-testid="compare-result">
      <div class="revline" data-testid="compare-summary">
        <span class="rev">
          <Icon name="commit" size="sm" />
          <span class="name">{shown.from}</span>
          {#if commits.from}<span class="oid">{shortOid(commits.from.oid)}</span><span class="msg">{commits.from.summary}</span>{/if}
        </span>
        <span class="arrow" aria-hidden="true"><Icon name="chevron-right" size="sm" /></span>
        <span class="rev">
          <Icon name="commit" size="sm" />
          <span class="name">{shown.to}</span>
          {#if commits.to}<span class="oid">{shortOid(commits.to.oid)}</span><span class="msg">{commits.to.summary}</span>{/if}
        </span>
        {#if running}<span class="status" data-testid="compare-running"><Spinner size="sm" label="Sending" /> Sending the requests at both revisions…</span>{/if}
        {#if runError}<span class="status bad" role="alert" data-testid="compare-run-error"><Icon name="error" size="sm" /> Answers not compared: {runError}</span>{/if}
        <span class="extension-actions"><ExtensionActions actions={rangeActions(shown.from, shown.to)} label="Range actions" /></span>
      </div>

      <CompareHeader {counts} {filter} ran={run !== null} onfilter={(f) => (filter = f)} />

      <div class="split">
        <div class="side">
          <div class="filters">
            <div class="chips" role="group" aria-label="Show">
              {#each FILTERS as f (f.id)}
                <Chip size="sm" label={f.label} selected={filter === f.id} data-testid="compare-filter-{f.id}" onclick={() => (filter = f.id)} />
              {/each}
            </div>
            <TextField size="sm" icon="search" type="search" placeholder="Filter requests" aria-label="Filter requests" data-testid="compare-search" bind:value={query} />
          </div>
          {#if list.length === 0}
            <div class="side-empty" data-testid="compare-empty">
              <EmptyState size="sm" icon="filter" title="No requests" text={emptyText} />
            </div>
          {:else}
            <CompareList rows={list} {selected} onselect={select} />
          {/if}
          <p class="keys"><Kbd keys="N" size="sm" /><Kbd keys="P" size="sm" /> next and previous change <span class="of">{list.length} of {rows.length}</span></p>
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
            <EmptyState
              icon={rows.length === 0 ? "success" : "compare"}
              title={rows.length === 0 ? "Nothing differs" : "Pick a request"}
              text={rows.length === 0 ? "Both revisions define the same requests." : "Choose a request on the left to see it side by side."}
            />
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
    gap: var(--rt-space-5);
    height: 100%;
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
  .revs {
    display: flex;
    align-items: flex-end;
    gap: var(--rt-space-4);
    flex: 1 1 440px;
    min-width: 0;
  }
  .revs > :global(.picker) {
    flex: 1 1 0;
  }
  .swap {
    display: inline-flex;
    flex: none;
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
  .send {
    display: inline-flex;
    white-space: nowrap;
  }
  .quick {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-3) var(--rt-space-4);
  }
  .hint {
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
  .extension-actions {
    margin-left: auto;
  }
  .revline {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-2) var(--rt-space-4);
    min-width: 0;
  }
  .rev {
    display: inline-flex;
    align-items: center;
    gap: var(--rt-space-3);
    min-width: 0;
    color: var(--rt-fg-muted);
  }
  .rev .name {
    color: var(--rt-fg);
    font: var(--rt-font-title);
    font-family: var(--rt-font-mono);
  }
  .oid {
    padding: 0 var(--rt-space-3);
    border-radius: var(--rt-radius-sm);
    background: var(--rt-surface-sunken);
    font: var(--rt-font-small);
    font-family: var(--rt-font-mono);
  }
  .msg {
    font: var(--rt-font-small);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 32ch;
  }
  .arrow {
    display: inline-flex;
    color: var(--rt-fg-subtle);
  }
  .status {
    display: inline-flex;
    align-items: center;
    gap: var(--rt-space-3);
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .status.bad {
    color: var(--rt-danger-fg);
  }
  .split {
    display: grid;
    grid-template-columns: minmax(260px, 360px) minmax(0, 1fr);
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
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--rt-space-3);
  }
  .side > :global(.viewport) {
    flex: 1;
  }
  .side-empty {
    flex: 1;
    padding: var(--rt-space-6) var(--rt-space-5);
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
</style>
