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
  import { Badge, Banner, Button, Card, TextField } from "../ui";
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
  <header>
    <p class="muted">
      Finds the first commit after Good where a request, test or assertion fails, by halving. Requests are read from each commit,
      nothing is checked out; they run with
      {app.environment ? `today's environment "${app.environment}"` : "no environment"}, its secrets, cookies and plugins.
    </p>
  </header>

  <form
    onsubmit={(e) => {
      e.preventDefault();
      void start();
    }}
  >
    <Card padding="md">
      <div class="config">
        <TextField label="Good" mono list="rt-bisect-revs" data-testid="bisect-good" bind:value={good} disabled={running} placeholder="a revision where all passes" />
        <TextField label="Bad" mono list="rt-bisect-revs" data-testid="bisect-bad" bind:value={bad} disabled={running} />
        <TextField label="Folder" mono data-testid="bisect-folder" bind:value={folder} placeholder="whole collection" disabled={running} />
        <datalist id="rt-bisect-revs">
          {#each branches as b (b.name)}<option value={b.name}></option>{/each}
        </datalist>
        <div class="buttons">
          {#if running}
            <Button data-testid="bisect-cancel" onclick={() => void cancel()}>Cancel</Button>
          {/if}
          <Button variant="primary" type="submit" icon="bisect" data-testid="bisect-start" loading={running} disabled={running || !good.trim() || !bad.trim()}>
            {running ? "Searching…" : "Start bisect"}
          </Button>
        </div>
      </div>
    </Card>
  </form>

  {#if error}
    <div data-testid="bisect-error"><Banner tone="danger">{error}</Banner></div>
  {/if}

  {#if running || steps.length > 0}
    <p class="progress" data-testid="bisect-progress" aria-live="polite">
      {steps.length} tested{left !== null ? `, ${left} left to look at` : ""}{cancelled ? " (cancelling…)" : ""}
    </p>
    <Card padding="none">
      <ol class="steps" data-testid="bisect-steps">
        {#each steps as step (step.commit.oid)}
          <li class="step" data-testid="bisect-step" data-verdict={step.verdict}>
            <span class="verdict"><Badge tone={step.verdict === "good" ? "ok" : step.verdict === "bad" ? "danger" : "warn"} value={VERDICT_LABEL[step.verdict]} /></span>
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
    </Card>
  {/if}

  {#if outcome}
    <div data-testid="bisect-result">
      {#if outcome.first_bad}
        {@const first = outcome.first_bad}
        {@const failed = outcome.steps.find((s) => s.commit.oid === first.oid)?.failed ?? []}
        <Banner tone="danger" title={bisectResult(outcome)}>
          {#snippet actions()}
            <Button size="sm" data-testid="bisect-open-compare" onclick={() => views.open({ kind: "compare", from: `${first.oid}~1`, to: first.oid })}>
              Compare with the commit before
            </Button>
          {/snippet}
          <span class="muted">{bisectProgress(outcome.steps, outcome.candidates)}</span>
          {#if failed.length > 0}
            <ul class="failed result-failed">
              {#each failed as f (f)}<li>{f}</li>{/each}
            </ul>
          {/if}
        </Banner>
      {:else}
        <Banner tone="info" title={bisectResult(outcome)}>
          <span class="muted">{bisectProgress(outcome.steps, outcome.candidates)}</span>
        </Banner>
      {/if}
    </div>
  {/if}
</section>

<style>
  .bisect {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-6);
    padding: var(--rt-space-6);
    min-height: 0;
  }
  p {
    margin: 0;
  }
  .muted {
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  header {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-2);
    max-width: 52rem;
  }
  .config {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--rt-space-4) var(--rt-space-6);
  }
  .buttons {
    display: flex;
    gap: var(--rt-space-4);
    margin-left: auto;
  }
  .progress {
    font: var(--rt-font-small);
    color: var(--rt-fg-muted);
  }
  .steps {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .step {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-2) var(--rt-space-5);
    padding: var(--rt-space-3) var(--rt-space-5);
  }
  .step + .step {
    border-top: 1px solid var(--rt-border);
  }
  .verdict {
    min-width: 4.5em;
  }
  .oid {
    font-family: var(--rt-font-mono);
    font: var(--rt-font-small);
    font-family: var(--rt-font-mono);
    color: var(--rt-fg-muted);
  }
  .summary {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .failed {
    flex-basis: 100%;
    margin: 0;
    padding-left: var(--rt-space-8);
    color: var(--rt-danger-fg);
    font: var(--rt-font-small);
    list-style: none;
  }
  .result-failed {
    padding-left: 0;
  }
</style>
