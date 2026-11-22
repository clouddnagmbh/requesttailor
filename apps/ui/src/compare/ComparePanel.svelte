<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The same requests run at two revisions, responses compared (wave 12, strand C). The
     requests come from each revision without a checkout; the environment, the secrets,
     cookies and plugins are today's. Per request: status, headers, JSON body by JSON
     Pointer, tests. -->
<script lang="ts">
  import { onMount } from "svelte";

  import type { GitBranch, RequestComparison, RunComparison } from "../commands/types";
  import { getApp } from "../state/app.svelte";
  import { arrow, changeSummary, comparisonCounts, shortOid, valueText } from "./model";

  let props: { from?: string; to?: string } = $props();

  const app = getApp();

  // svelte-ignore state_referenced_locally: the props only seed the fields.
  let from = $state(props.from ?? "HEAD~1");
  // svelte-ignore state_referenced_locally: the props only seed the fields.
  let to = $state(props.to ?? "HEAD");
  let folder = $state("");
  let onlyChanged = $state(true);
  let running = $state(false);
  let error = $state<string | null>(null);
  let result = $state<RunComparison | null>(null);
  let open = $state<Record<string, boolean>>({});
  let branches = $state<GitBranch[]>([]);

  const counts = $derived(result ? comparisonCounts(result) : null);
  const rows = $derived(result ? result.requests.filter((r) => !onlyChanged || !r.same) : []);

  onMount(() => {
    void app.backend.git
      .branches()
      .then((b) => (branches = b))
      .catch(() => {});
    // Opened with both revisions (from the bisect result, or the palette): compare at once.
    if (props.from && props.to) void start();
  });

  async function start() {
    if (running) return;
    running = true;
    error = null;
    result = null;
    open = {};
    try {
      result = await app.backend.compare.compare(
        { from: from.trim(), to: to.trim(), folder: folder.trim() || null },
        { environment: app.environment, vars: [] },
      );
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      running = false;
    }
  }

  function toggle(r: RequestComparison) {
    open[r.path] = !open[r.path];
  }
</script>

<section class="compare" data-testid="compare-panel" aria-label="Compare revisions">
  <form
    class="config"
    onsubmit={(e) => {
      e.preventDefault();
      void start();
    }}
  >
    <label>
      From
      <input list="rt-compare-revs" data-testid="compare-from" bind:value={from} disabled={running} spellcheck="false" />
    </label>
    <label>
      To
      <input list="rt-compare-revs" data-testid="compare-to" bind:value={to} disabled={running} spellcheck="false" />
    </label>
    <label>
      Folder
      <input data-testid="compare-folder" bind:value={folder} placeholder="whole collection" disabled={running} spellcheck="false" />
    </label>
    <datalist id="rt-compare-revs">
      {#each branches as b (b.name)}<option value={b.name}></option>{/each}
    </datalist>
    <span class="spacer"></span>
    {#if running}
      <button type="button" class="button" data-testid="compare-cancel" onclick={() => void app.backend.compare.cancel()}>
        Cancel
      </button>
    {/if}
    <button type="submit" class="button primary" data-testid="compare-start" disabled={running || !from.trim() || !to.trim()}>
      {running ? "Comparing…" : "Compare"}
    </button>
  </form>
  <p class="hint">
    Requests are read from each revision, nothing is checked out. They run with
    {app.environment ? `today's environment "${app.environment}"` : "no environment"}, its secrets, cookies and plugins.
  </p>

  {#if error}
    <p class="error" role="alert" data-testid="compare-error">{error}</p>
  {/if}

  {#if result && counts}
    <div class="summary" data-testid="compare-summary">
      <span class="rev" title={result.from.summary}>{shortOid(result.from.oid)} <span class="muted">{result.from.summary}</span></span>
      <span class="muted">→</span>
      <span class="rev" title={result.to.summary}>{shortOid(result.to.oid)} <span class="muted">{result.to.summary}</span></span>
      <span class="spacer"></span>
      <span class={counts.changed > 0 ? "bad" : "ok"} data-testid="compare-changed">{counts.changed} changed</span>
      <span class="muted" data-testid="compare-same">{counts.same} unchanged</span>
      <span class="muted">
        tests failing: {result.from_summary.tests_failed + result.from_summary.assertions_failed} →
        {result.to_summary.tests_failed + result.to_summary.assertions_failed}
      </span>
      <label class="check">
        <input type="checkbox" data-testid="compare-only-changed" bind:checked={onlyChanged} /> Changed only
      </label>
    </div>

    {#if rows.length === 0}
      <p class="muted" data-testid="compare-empty">
        {counts.total === 0 ? "No requests to compare." : "Every request answers the same at both revisions."}
      </p>
    {/if}

    <ul class="list" data-testid="compare-list">
      {#each rows as r (r.path)}
        <li class="item" data-testid="compare-row" data-path={r.path} data-same={r.same}>
          <button class="row" aria-expanded={open[r.path] === true} onclick={() => toggle(r)}>
            <span class="state {r.same ? 'ok' : 'bad'}" aria-label={r.same ? "Unchanged" : "Changed"}>{r.same ? "✓" : "●"}</span>
            <span class="name">{r.name}</span>
            <span class="path muted">{r.path}</span>
            <span class="what muted">{changeSummary(r)}</span>
          </button>
          {#if open[r.path]}
            <div class="detail" data-testid="compare-detail">
              {#if r.status}
                <h4>Status</h4>
                <p data-testid="compare-status">{arrow(r.status.old, r.status.new)}</p>
              {/if}
              {#if r.error}
                <h4>Error</h4>
                <p class="bad" data-testid="compare-error-change">{arrow(r.error.old, r.error.new)}</p>
              {/if}
              {#if r.headers.length > 0}
                <h4>Headers</h4>
                <table data-testid="compare-headers">
                  <tbody>
                    {#each r.headers as h (h.name)}
                      <tr><td>{h.name}</td><td class="old">{h.old ?? "none"}</td><td class="new">{h.new ?? "none"}</td></tr>
                    {/each}
                  </tbody>
                </table>
              {/if}
              {#if r.body.length > 0 || r.body_text}
                <h4>Body</h4>
                {#if r.body_text}<p class="muted" data-testid="compare-body-text">The bodies are not JSON and differ.</p>{/if}
                <table data-testid="compare-body">
                  <tbody>
                    {#each r.body as b (b.pointer)}
                      <tr>
                        <td class="mono">{b.pointer === "" ? "(whole body)" : b.pointer}</td>
                        <td class="old mono">{valueText(b.old)}</td>
                        <td class="new mono">{valueText(b.new)}</td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              {/if}
              {#if r.tests.length > 0}
                <h4>Tests</h4>
                <table data-testid="compare-tests">
                  <tbody>
                    {#each r.tests as t (t.name)}
                      <tr>
                        <td>{t.name}</td>
                        <td class={t.old === false ? "bad" : "ok"}>{t.old === null ? "not run" : t.old ? "passed" : "failed"}</td>
                        <td class={t.new === false ? "bad" : "ok"}>{t.new === null ? "not run" : t.new ? "passed" : "failed"}</td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              {/if}
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .compare {
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
    padding: var(--rt-gap) 12px;
    min-height: 0;
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
  input:not([type="checkbox"]) {
    font: inherit;
    font-family: var(--rt-font-mono);
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    padding: 2px 6px;
    width: 12em;
  }
  .spacer {
    flex: 1;
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
  .hint,
  .muted {
    color: var(--rt-fg-muted);
  }
  .hint {
    margin: 0;
    font-size: var(--rt-size-small);
  }
  .error {
    margin: 0;
    color: var(--rt-danger);
  }
  .ok {
    color: var(--rt-ok);
  }
  .bad {
    color: var(--rt-danger);
  }
  .summary {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 4px 14px;
  }
  .rev {
    font-family: var(--rt-font-mono);
    font-size: var(--rt-size-small);
  }
  .check {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
  }
  .item + .item {
    border-top: 1px solid var(--rt-border);
  }
  .row {
    display: flex;
    align-items: baseline;
    gap: 10px;
    width: 100%;
    padding: 6px 8px;
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .row:hover {
    background: var(--rt-bg-hover);
  }
  .row:focus-visible {
    outline: 2px solid var(--rt-focus);
    outline-offset: -2px;
  }
  .state {
    width: 1.2em;
    text-align: center;
  }
  .path,
  .what {
    font-size: var(--rt-size-small);
  }
  .what {
    margin-left: auto;
  }
  .detail {
    padding: 0 8px 8px 32px;
    background: var(--rt-bg-panel);
  }
  h4 {
    margin: 8px 0 2px;
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  p {
    margin: 0;
  }
  table {
    border-collapse: collapse;
    font-size: var(--rt-size-small);
  }
  td {
    padding: 2px 12px 2px 0;
    vertical-align: top;
  }
  .mono {
    font-family: var(--rt-font-mono);
  }
  .old {
    color: var(--rt-danger);
  }
  .new {
    color: var(--rt-ok);
  }
</style>
