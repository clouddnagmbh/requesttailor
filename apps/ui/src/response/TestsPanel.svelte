<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The response's "Tests" tab (wave 7, strand C): results of test scripts and declarative
     assertions, failed first, with their messages; script notes (not trusted, failed,
     skipped) on top. -->
<script lang="ts">
  import type { SendReport } from "../commands/types";
  import { scriptNotes, testSummary } from "./tests";

  let { report }: { report: SendReport } = $props();

  const summary = $derived(testSummary(report));
  const notes = $derived(scriptNotes(report));
  const skipped = $derived(report.flow?.kind === "skip");
</script>

<div class="tests" data-testid="tests-panel">
  {#if notes.length > 0 || skipped}
    <ul class="notes" aria-label="Script notes">
      {#if skipped}<li class="warning" data-testid="tests-skipped">A pre-request script skipped this request.</li>{/if}
      {#each notes as n, i (i)}
        <li class={n.level} data-testid="tests-note" data-level={n.level}>{n.text}</li>
      {/each}
    </ul>
  {/if}
  {#if summary.total === 0}
    <p class="empty" data-testid="tests-empty">
      No tests or assertions ran. Add them in the request's Scripts (<code>test(…)</code>) or Assertions tab.
    </p>
  {:else}
    <p class="summary" data-testid="tests-summary" class:failed={summary.passed < summary.total}>
      {summary.passed} of {summary.total} passed{summary.passed < summary.total ? `, ${summary.total - summary.passed} failed` : ""}
    </p>
    <ul class="results" aria-label="Results">
      {#each summary.rows as r, i (i)}
        <li class:pass={r.passed} class:fail={!r.passed} data-testid="test-result" data-passed={r.passed} data-kind={r.kind}>
          <span class="mark" aria-hidden="true">{r.passed ? "✓" : "✗"}</span>
          <span class="sr-only">{r.passed ? "Passed" : "Failed"}:</span>
          <span class="kind">{r.kind === "test" ? "Test" : "Assert"}</span>
          <span class="title" class:mono={r.kind === "assertion"}>{r.title}</span>
          {#if r.origin && r.origin !== "request"}<span class="origin">{r.origin}</span>{/if}
          {#if r.detail}<span class="detail" data-testid="test-detail">{r.detail}</span>{/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .tests {
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
    padding: 4px var(--rt-gap);
    font-size: var(--rt-size-small);
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .notes li {
    padding: 1px 0 1px var(--rt-gap);
    margin-bottom: 4px;
    border-left: 3px solid var(--rt-accent);
  }
  .notes li.warning {
    border-color: var(--rt-warn);
  }
  .notes li.error {
    border-color: var(--rt-danger);
    color: var(--rt-danger);
  }
  .summary {
    margin: 0 0 4px;
    color: var(--rt-ok);
    font-weight: 600;
  }
  .summary.failed {
    color: var(--rt-danger);
  }
  .results li {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 0 var(--rt-gap);
    padding: 2px 0;
    border-bottom: 1px solid var(--rt-border);
  }
  .mark {
    width: 1em;
    font-weight: 700;
  }
  .pass .mark {
    color: var(--rt-ok);
  }
  .fail .mark,
  .fail .title {
    color: var(--rt-danger);
  }
  .kind,
  .origin {
    color: var(--rt-fg-muted);
  }
  .title {
    word-break: break-word;
  }
  .mono {
    font-family: var(--rt-font-mono);
  }
  .detail {
    flex-basis: 100%;
    padding-left: calc(1em + var(--rt-gap));
    font-family: var(--rt-font-mono);
    white-space: pre-wrap;
    word-break: break-word;
    color: var(--rt-fg-muted);
  }
  .empty {
    margin: 0;
    color: var(--rt-fg-muted);
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
