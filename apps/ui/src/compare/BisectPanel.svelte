<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Bisect with progress (wave 12, strand C): the first commit between a good and a bad
     revision at which a request, test or assertion fails. Each candidate runs from Git
     without a checkout, with today's environment and secrets. -->
<script lang="ts">
  import { onMount } from "svelte";

  import type { BisectOutcome, BisectStep, GitBranch } from "../commands/types";
  import { getGitViews } from "../git/views.svelte";
  import { getApp } from "../state/app.svelte";
  import { bisectProgress, bisectResult, shortOid, VERDICT_LABEL } from "./model";

  let props: { good?: string; bad?: string } = $props();

  const app = getApp();
  const views = getGitViews();

  // svelte-ignore state_referenced_locally: the props only seed the fields.
  let good = $state(props.good ?? "");
  // svelte-ignore state_referenced_locally: the props only seed the fields.
  let bad = $state(props.bad ?? "HEAD");
  let folder = $state("");
  let running = $state(false);
  let cancelled = $state(false);
  let error = $state<string | null>(null);
  let steps = $state<BisectStep[]>([]);
  let outcome = $state<BisectOutcome | null>(null);
  let branches = $state<GitBranch[]>([]);

  onMount(() => {
    void app.backend.git
      .branches()
      .then((b) => (branches = b))
      .catch(() => {});
  });

  async function start() {
    if (running) return;
    running = true;
    cancelled = false;
    error = null;
    outcome = null;
    steps = [];
    try {
      const result = await app.backend.compare.bisect(
        { good: good.trim(), bad: bad.trim(), folder: folder.trim() || null },
        { environment: app.environment, vars: [] },
        (step) => steps.push(step),
      );
      // A cancelled search has no answer, but the steps so far stay on screen.
      outcome = cancelled ? null : result;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      running = false;
    }
  }

  async function cancel() {
    cancelled = true;
    await app.backend.compare.cancel();
  }

  const left = $derived(steps.length > 0 ? steps[steps.length - 1]!.remaining : null);
</script>

<section class="bisect" data-testid="bisect-panel" aria-label="Bisect">
  <form
    class="config"
    onsubmit={(e) => {
      e.preventDefault();
      void start();
    }}
  >
    <label>
      Good
      <input list="rt-bisect-revs" data-testid="bisect-good" bind:value={good} disabled={running} placeholder="a revision where all passes" spellcheck="false" />
    </label>
    <label>
      Bad
      <input list="rt-bisect-revs" data-testid="bisect-bad" bind:value={bad} disabled={running} spellcheck="false" />
    </label>
    <label>
      Folder
      <input data-testid="bisect-folder" bind:value={folder} placeholder="whole collection" disabled={running} spellcheck="false" />
    </label>
    <datalist id="rt-bisect-revs">
      {#each branches as b (b.name)}<option value={b.name}></option>{/each}
    </datalist>
    <span class="spacer"></span>
    {#if running}
      <button type="button" class="button" data-testid="bisect-cancel" onclick={() => void cancel()}>Cancel</button>
    {/if}
    <button type="submit" class="button primary" data-testid="bisect-start" disabled={running || !good.trim() || !bad.trim()}>
      {running ? "Searching…" : "Start bisect"}
    </button>
  </form>
  <p class="hint">
    Finds the first commit after Good where a request, test or assertion fails, by halving. Requests are read from each commit,
    nothing is checked out; they run with
    {app.environment ? `today's environment "${app.environment}"` : "no environment"}, its secrets, cookies and plugins.
  </p>

  {#if error}
    <p class="error" role="alert" data-testid="bisect-error">{error}</p>
  {/if}

  {#if running || steps.length > 0}
    <p class="progress" data-testid="bisect-progress" aria-live="polite">
      {steps.length} tested{left !== null ? `, ${left} left to look at` : ""}{cancelled ? " (cancelling…)" : ""}
    </p>
    <ol class="steps" data-testid="bisect-steps">
      {#each steps as step (step.commit.oid)}
        <li class="step" data-testid="bisect-step" data-verdict={step.verdict}>
          <span class="verdict {step.verdict}">{VERDICT_LABEL[step.verdict]}</span>
          <span class="oid">{shortOid(step.commit.oid)}</span>
          <span class="summary">{step.commit.summary}</span>
          {#if step.failed.length > 0}
            <ul class="failed">
              {#each step.failed as f (f)}<li>{f}</li>{/each}
            </ul>
          {/if}
        </li>
      {/each}
    </ol>
  {/if}

  {#if outcome}
    <div class="result {outcome.first_bad ? 'found' : 'none'}" data-testid="bisect-result" role="status">
      <strong>{bisectResult(outcome)}</strong>
      <span class="muted">{bisectProgress(outcome.steps, outcome.candidates)}</span>
      {#if outcome.first_bad}
        {@const first = outcome.first_bad}
        {@const failed = outcome.steps.find((s) => s.commit.oid === first.oid)?.failed ?? []}
        {#if failed.length > 0}
          <ul class="failed">
            {#each failed as f (f)}<li>{f}</li>{/each}
          </ul>
        {/if}
        <button
          class="button"
          data-testid="bisect-open-compare"
          onclick={() => views.open({ kind: "compare", from: `${first.oid}~1`, to: first.oid })}
        >
          Compare with the commit before
        </button>
      {/if}
    </div>
  {/if}
</section>

<style>
  .bisect {
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
  input {
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
    align-self: flex-start;
  }
  .config .button {
    align-self: auto;
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
  p {
    margin: 0;
  }
  .hint {
    font-size: var(--rt-size-small);
  }
  .error {
    color: var(--rt-danger);
  }
  .steps {
    list-style: none;
    margin: 0;
    padding: 0;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
  }
  .step {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 2px 10px;
    padding: 4px 8px;
  }
  .step + .step {
    border-top: 1px solid var(--rt-border);
  }
  .verdict {
    min-width: 4.5em;
    font-weight: 600;
  }
  .verdict.good {
    color: var(--rt-ok);
  }
  .verdict.bad {
    color: var(--rt-danger);
  }
  .verdict.skip {
    color: var(--rt-warn);
  }
  .oid {
    font-family: var(--rt-font-mono);
    font-size: var(--rt-size-small);
  }
  .failed {
    flex-basis: 100%;
    margin: 0;
    padding-left: 5.5em;
    color: var(--rt-danger);
    font-size: var(--rt-size-small);
    list-style: none;
  }
  .result {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: var(--rt-gap) 12px;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg-panel);
  }
  .result.found {
    border-color: var(--rt-danger);
  }
  .result .failed {
    padding-left: 0;
  }
</style>
