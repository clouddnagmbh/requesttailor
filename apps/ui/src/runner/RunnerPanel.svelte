<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The collection runner in the main area (wave 7, strand D): configuration (iterations,
     delay, data file with a preview, bail; the environment is the active one), progress,
     summary, and the results as a windowed list (thousands of steps stay smooth). The
     selected step's tests, assertions, logs and error show below the list. Opened from a
     folder's "Run" button, the header button "Run" and the palette ("Run collection"). -->
<script lang="ts">
  import { tick } from "svelte";

  import { describeError, formatDuration, statusClass } from "../response/format";
  import ExtensionActions from "../shell/ExtensionActions.svelte";
  import { extensionRunActions } from "../shell/extension-actions";
  import { getExtensions } from "../shell/extension-tabs";
  import MethodBadge from "../shell/MethodBadge.svelte";
  import { getApp } from "../state/app.svelte";
  import { dataPreview, progressText, scrollToShow, stepCounts, stepState, stepTime, windowRange } from "./model";
  import type { RunnerState } from "./runner.svelte";

  let { runner }: { runner: RunnerState } = $props();

  const app = getApp();
  const ROW = 28;

  let listEl = $state<HTMLElement>();
  let fileInput = $state<HTMLInputElement>();
  let scrollTop = $state(0);
  let viewport = $state(0);
  let selected = $state<number | null>(null);
  /** Keep the newest step in view while the run goes on, until the user scrolls up. */
  let follow = $state(true);

  const view = $derived.by(() => {
    void runner.version;
    const r = runner.results;
    return {
      count: r.steps.length,
      total: r.total,
      current: r.current,
      counts: { ...r.counts },
      summary: r.summary,
    };
  });
  const range = $derived(windowRange(scrollTop, viewport, ROW, view.count));
  const visible = $derived.by(() => {
    void runner.version;
    return runner.results.steps.slice(range[0], range[1]).map((step, k) => ({ step, index: range[0] + k }));
  });
  const detail = $derived.by(() => {
    void runner.version;
    return selected === null ? null : (runner.results.steps[selected] ?? null);
  });
  const preview = $derived(runner.data ? dataPreview(runner.data) : null);
  const iterationsShown = $derived(runner.data ? runner.data.length : Math.max(1, Math.floor(runner.iterations || 1)));
  const sameTarget = $derived(runner.ran !== null &&
      runner.target !== null &&
      runner.ran.folder === runner.target.folder &&
      runner.ran.collection === runner.target.collection);
  // Wave 16: actions of extensions on a finished run.
  const extensions = getExtensions();
  const runActions = $derived.by(() => {
    void runner.version;
    const summary = runner.results.summary;
    if (runner.running || !summary || !runner.ran || !app.activeCollection) return [];
    const events = [
      { kind: "started" as const, total: runner.results.total },
      ...runner.results.steps.map((step) => ({ kind: "step" as const, step })),
      { kind: "finished" as const, summary },
    ];
    return extensionRunActions(extensions, { kind: "run", collection: app.activeCollection, folder: runner.ran.folder ?? "", summary, events });
  });
  const hasResults = $derived(sameTarget && (view.count > 0 || view.summary !== null || runner.running));

  // New run: back to the top, nothing selected.
  let lastResults: unknown = null;
  $effect(() => {
    void runner.version;
    if (runner.results !== lastResults) {
      lastResults = runner.results;
      selected = null;
      follow = true;
      scrollTop = 0;
      if (listEl) listEl.scrollTop = 0;
    }
  });

  // Follow the newest step.
  $effect(() => {
    const count = view.count;
    if (!follow || !listEl || !runner.running) return;
    const bottom = Math.max(0, count * ROW - viewport);
    if (listEl.scrollTop < bottom) listEl.scrollTop = bottom;
  });

  function onscroll() {
    if (!listEl) return;
    scrollTop = listEl.scrollTop;
    follow = listEl.scrollTop + listEl.clientHeight >= listEl.scrollHeight - ROW;
  }

  async function select(index: number) {
    if (view.count === 0) return;
    const i = Math.max(0, Math.min(index, view.count - 1));
    selected = i;
    follow = false;
    if (listEl) {
      listEl.scrollTop = scrollToShow(i, listEl.scrollTop, listEl.clientHeight, ROW);
      scrollTop = listEl.scrollTop;
    }
    await tick();
  }

  function onkeydown(e: KeyboardEvent) {
    const page = Math.max(1, Math.floor(viewport / ROW) - 1);
    const at = selected ?? -1;
    const keys: Record<string, number> = {
      ArrowDown: at + 1,
      ArrowUp: Math.max(0, at - 1),
      Home: 0,
      End: view.count - 1,
      PageDown: at + page,
      PageUp: Math.max(0, at - page),
    };
    if (e.key in keys) {
      e.preventDefault();
      void select(keys[e.key]!);
    } else if (e.key === "Escape" && selected !== null) {
      e.preventDefault();
      selected = null;
    }
  }

  async function onfile(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    await runner.loadData({ name: file.name, data: new Uint8Array(await file.arrayBuffer()) });
    input.value = "";
  }

  const stateLabel = { passed: "Passed", failed: "Failed", skipped: "Skipped" } as const;
  const stateIcon = { passed: "✓", failed: "✗", skipped: "–" } as const;
</script>

{#if runner.target}
  <section class="runner" data-testid="runner-panel" aria-labelledby="rt-runner-title">
    <div class="bar">
      <h2 id="rt-runner-title" data-testid="runner-title">
        Run {runner.target.folder === null ? "collection" : "folder"}: {runner.target.name}
      </h2>
      <span class="spacer"></span>
      <button class="close" data-testid="runner-close" aria-label="Close runner" onclick={() => runner.close()}>×</button>
    </div>

    <form
      class="config"
      onsubmit={(e) => {
        e.preventDefault();
        void runner.start();
      }}
    >
      <label>
        Iterations
        <input
          type="number"
          min="1"
          step="1"
          data-testid="runner-iterations"
          bind:value={runner.iterations}
          disabled={runner.running || runner.data !== null}
          title={runner.data ? "One iteration per data row" : ""}
        />
      </label>
      <label>
        Delay (ms)
        <input type="number" min="0" step="10" data-testid="runner-delay" bind:value={runner.delayMs} disabled={runner.running} />
      </label>
      <label class="check">
        <input type="checkbox" data-testid="runner-bail" bind:checked={runner.bail} disabled={runner.running} />
        Stop at the first failure
      </label>
      <span class="env" data-testid="runner-environment">
        Environment: <strong>{app.environment ?? "none"}</strong>
      </span>
      <span class="data">
        <input
          bind:this={fileInput}
          class="file"
          type="file"
          accept=".csv,.json,text/csv,application/json"
          data-testid="runner-data"
          aria-label="Data file (CSV or JSON)"
          onchange={onfile}
          disabled={runner.running}
        />
        <button type="button" class="button" disabled={runner.running} onclick={() => fileInput?.click()}>
          {runner.dataName ? "Change data file…" : "Data file…"}
        </button>
        {#if runner.dataName}
          <span data-testid="runner-data-name">{runner.dataName}: {runner.data?.length ?? 0} rows</span>
          <button type="button" class="link" data-testid="runner-data-clear" disabled={runner.running} onclick={() => runner.clearData()}>
            Remove
          </button>
        {/if}
      </span>
      <span class="spacer"></span>
      {#if runner.running}
        <button type="button" class="button" data-testid="runner-cancel" onclick={() => runner.cancel()}>Cancel</button>
      {/if}
      <button type="submit" class="button primary" data-testid="runner-start" disabled={runner.running}>
        {hasResults ? "Run again" : "Run"}
      </button>
    </form>

    {#if runner.dataError}
      <p class="error" role="alert" data-testid="runner-data-error">{runner.dataError}</p>
    {/if}
    {#if preview}
      <div class="preview" data-testid="runner-data-preview">
        <table>
          <thead>
            <tr>
              <th scope="col">#</th>
              {#each preview.columns as c (c)}<th scope="col">{c}</th>{/each}
            </tr>
          </thead>
          <tbody>
            {#each preview.rows as row, i (i)}
              <tr data-testid="runner-data-row">
                <td>{i + 1}</td>
                {#each row as cell, j (j)}<td>{cell}</td>{/each}
              </tr>
            {/each}
          </tbody>
        </table>
        {#if (runner.data?.length ?? 0) > preview.rows.length}
          <p class="muted">… and {(runner.data?.length ?? 0) - preview.rows.length} more rows</p>
        {/if}
      </div>
    {/if}
    {#if runner.error}
      <p class="error" role="alert" data-testid="runner-error">{runner.error}</p>
    {/if}

    {#if hasResults}
      <div
        class="summary"
        data-testid="runner-summary"
        data-requests={view.counts.requests}
        data-passed={view.counts.passed}
        data-failed={view.counts.failed}
        data-skipped={view.counts.skipped}
        data-stopped={view.summary?.stopped ?? false}
        data-finished={view.summary !== null}
        aria-live="polite"
      >
        <span data-testid="runner-progress">
          {#if runner.running}
            Running {progressText(view.count, view.total)}{#if view.current}: {view.current.request_path}{/if}
          {:else if view.summary}
            {view.summary.stopped ? "Stopped" : "Finished"} after {view.count} requests in {formatDuration(view.summary.duration_ms)}
          {/if}
        </span>
        <span class="ok">{view.counts.passed} passed</span>
        <span class={view.counts.failed > 0 ? "bad" : "muted"}>{view.counts.failed} failed</span>
        {#if view.counts.skipped > 0}<span class="muted">{view.counts.skipped} skipped</span>{/if}
        <span class="muted">
          Tests {view.counts.tests_passed}/{view.counts.tests_passed + view.counts.tests_failed}, assertions
          {view.counts.assertions_passed}/{view.counts.assertions_passed + view.counts.assertions_failed}
        </span>
        {#if iterationsShown > 1}<span class="muted">{iterationsShown} iterations</span>{/if}
        <span class="push"><ExtensionActions actions={runActions} label="Run actions" variant="ghost" /></span>
      </div>

      <div class="results">
        <div
          class="list"
          role="listbox"
          tabindex="0"
          aria-label="Run results"
          aria-activedescendant={selected !== null ? `rt-run-step-${selected}` : undefined}
          data-testid="runner-results"
          data-count={view.count}
          bind:this={listEl}
          bind:clientHeight={viewport}
          {onscroll}
          {onkeydown}
        >
          <div class="spacer-rows" style:height="{view.count * ROW}px">
            {#each visible as { step, index } (index)}
              {@const state = stepState(step)}
              {@const n = stepCounts(step)}
              {@const time = stepTime(step)}
              {@const status = step.report.response?.status}
              <div
                id="rt-run-step-{index}"
                class="row {state}"
                class:selected={selected === index}
                role="option"
                tabindex="-1"
                aria-selected={selected === index}
                data-testid="runner-step"
                data-index={index}
                data-state={state}
                data-path={step.request_path}
                style:top="{index * ROW}px"
                onclick={() => select(index)}
                onkeydown={() => {}}
              >
                <span class="state" title={stateLabel[state]} aria-label={stateLabel[state]}>{stateIcon[state]}</span>
                <span class="num">{index + 1}</span>
                {#if iterationsShown > 1}<span class="iter" title="Iteration">#{step.iteration + 1}</span>{/if}
                <span class="method"><MethodBadge method={step.report.request?.method ?? "GET"} /></span>
                <span class="name" title={step.request_path}>{step.request_name}</span>
                <span class="status {status ? statusClass(status) : ''}">{status ?? (step.skipped ? "skipped" : "—")}</span>
                <span class="time">{time !== null ? formatDuration(time) : ""}</span>
                <span class="checks" title="Tests and assertions passed of all">
                  {#if n.tests[0] + n.tests[1] + n.assertions[0] + n.assertions[1] > 0}
                    {n.tests[0] + n.assertions[0]}/{n.tests[0] + n.tests[1] + n.assertions[0] + n.assertions[1]}
                  {/if}
                </span>
              </div>
            {/each}
          </div>
        </div>

        {#if detail}
          {@const n = stepCounts(detail)}
          <div class="detail" data-testid="runner-step-details">
            <h3>
              {detail.request_name}
              <span class="muted">· iteration {detail.iteration + 1} · {detail.request_path}</span>
            </h3>
            {#if detail.report.request}
              <p class="mono">{detail.report.request.method} {detail.report.request.url}</p>
            {/if}
            {#if detail.report.error}
              <p class="error" data-testid="runner-step-error">{describeError(detail.report.error)}</p>
            {/if}
            {#each detail.report.notes ?? [] as note, i (i)}
              <p class={note.level === "error" ? "error" : "muted"} data-testid="runner-note">{note.source}: {note.text}</p>
            {/each}
            {#if (detail.report.tests ?? []).length > 0}
              <h4>Tests ({n.tests[0]}/{n.tests[0] + n.tests[1]})</h4>
              <ul>
                {#each detail.report.tests ?? [] as t, i (i)}
                  <li class={t.passed ? "ok" : "bad"} data-testid="runner-test" data-passed={t.passed}>
                    {t.passed ? "✓" : "✗"}
                    {t.name}{#if t.error}: <span class="mono">{t.error}</span>{/if}
                  </li>
                {/each}
              </ul>
            {/if}
            {#if (detail.report.assertions ?? []).length > 0}
              <h4>Assertions ({n.assertions[0]}/{n.assertions[0] + n.assertions[1]})</h4>
              <ul>
                {#each detail.report.assertions ?? [] as a, i (i)}
                  <li class={a.passed ? "ok" : "bad"} data-testid="runner-assertion" data-passed={a.passed}>
                    {a.passed ? "✓" : "✗"}
                    <span class="mono">{a.expression} {a.operator} {a.expected}</span>
                    {#if !a.passed}
                      — got <span class="mono">{a.actual ?? "nothing"}</span>{#if a.error}: {a.error}{/if}
                    {/if}
                  </li>
                {/each}
              </ul>
            {/if}
            {#if (detail.report.logs ?? []).length > 0}
              <h4>Logs</h4>
              <pre class="logs">{#each detail.report.logs ?? [] as l, i (i)}<span data-testid="runner-log" class="log {l.level}">{l.text}
</span>{/each}</pre>
            {/if}
            {#if (detail.report.var_changes ?? []).length > 0}
              <h4>Variables set</h4>
              <ul>
                {#each detail.report.var_changes ?? [] as c, i (i)}
                  <li class="mono">{c.name} = {c.value ?? "(unset)"}</li>
                {/each}
              </ul>
            {/if}
            {#if !detail.report.error && (detail.report.tests ?? []).length === 0 && (detail.report.assertions ?? []).length === 0 && (detail.report.logs ?? []).length === 0}
              <p class="muted">{detail.skipped ? "Skipped by a script." : "No tests or assertions."}</p>
            {/if}
          </div>
        {/if}
      </div>
    {:else}
      <p class="hint">
        Runs the requests {runner.target.folder === null ? "of the collection" : "of this folder"} in order with the
        active environment. Variables that scripts set carry to the next requests of the run.
      </p>
    {/if}
  </section>
{/if}

<style>
  .runner {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
    padding: 0 12px var(--rt-gap);
  }
  .bar {
    display: flex;
    align-items: center;
    border-bottom: 1px solid var(--rt-border);
  }
  h2 {
    margin: 6px 0;
    font-size: var(--rt-size);
  }
  .spacer {
    flex: 1;
  }
  .close {
    border: 0;
    background: transparent;
    color: var(--rt-fg-muted);
    font-size: 18px;
    cursor: pointer;
  }
  .config {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-gap) 16px;
  }
  .config label {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .config input[type="number"] {
    width: 6em;
    font: inherit;
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    padding: 2px 6px;
  }
  .data {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
  .file {
    position: absolute;
    width: 1px;
    height: 1px;
    opacity: 0;
    pointer-events: none;
  }
  .button {
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg);
    color: var(--rt-fg);
    cursor: pointer;
  }
  .button.primary:not(:disabled) {
    background: var(--rt-accent);
    border-color: var(--rt-accent);
    color: var(--rt-accent-fg);
  }
  .button:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .link {
    border: 0;
    background: none;
    color: var(--rt-accent);
    cursor: pointer;
    padding: 0;
    font: inherit;
  }
  .preview {
    max-height: 160px;
    overflow: auto;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
  }
  table {
    border-collapse: collapse;
    font-size: var(--rt-size-small);
  }
  th,
  td {
    padding: 2px 8px;
    border-bottom: 1px solid var(--rt-border);
    text-align: left;
    white-space: nowrap;
  }
  .push {
    margin-left: auto;
  }
  .summary {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 14px;
    align-items: baseline;
  }
  .results {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
  }
  .list {
    flex: 1;
    min-height: 120px;
    overflow: auto;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    contain: strict;
  }
  .list:focus-visible {
    outline: 2px solid var(--rt-focus);
    outline-offset: -2px;
  }
  .spacer-rows {
    position: relative;
  }
  .row {
    position: absolute;
    left: 0;
    right: 0;
    height: 28px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 8px;
    white-space: nowrap;
    cursor: default;
  }
  .row:hover {
    background: var(--rt-bg-hover);
  }
  .row.selected {
    background: var(--rt-bg-selected);
  }
  .state {
    width: 1em;
    text-align: center;
  }
  .row.passed .state,
  .ok {
    color: var(--rt-ok);
  }
  .row.failed .state,
  .bad,
  .error {
    color: var(--rt-danger);
  }
  .row.skipped .state,
  .muted,
  .hint,
  .num,
  .iter,
  .time,
  .checks {
    color: var(--rt-fg-muted);
  }
  .method {
    flex: none;
    width: 3.6em;
  }
  .num {
    width: 3.5em;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .status,
  .time,
  .checks {
    font-variant-numeric: tabular-nums;
  }
  .status.ok {
    color: var(--rt-ok);
  }
  .status.client-error,
  .status.server-error {
    color: var(--rt-danger);
  }
  .status.redirect {
    color: var(--rt-warn);
  }
  .time {
    width: 5em;
    text-align: right;
  }
  .checks {
    width: 3.5em;
    text-align: right;
  }
  .detail {
    max-height: 40%;
    overflow: auto;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    padding: 6px 10px;
    background: var(--rt-bg-panel);
  }
  .detail h3 {
    margin: 0 0 4px;
    font-size: var(--rt-size);
  }
  .detail h4 {
    margin: 8px 0 2px;
    font-size: var(--rt-size-small);
    text-transform: uppercase;
    color: var(--rt-fg-muted);
  }
  .detail ul {
    margin: 0;
    padding-left: 1.2em;
    list-style: none;
  }
  .detail p {
    margin: 2px 0;
  }
  .mono,
  .logs {
    font-family: var(--rt-font-mono);
    font-size: var(--rt-size-small);
  }
  .logs {
    margin: 0;
    white-space: pre-wrap;
  }
  .log.warn {
    color: var(--rt-warn);
  }
  .log.error {
    color: var(--rt-danger);
  }
</style>
