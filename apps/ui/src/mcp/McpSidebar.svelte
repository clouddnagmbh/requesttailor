<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Sidebar view "MCP" (wave 8, strand D): a new connection, and the collection's saved MCP
     tests (*.mcp.yaml) to open, run one by one or all, with pass/fail of the last run. Tests
     that start a local command (stdio) run only after the user agreed once per collection. -->
<script lang="ts">
  import { getApp } from "../state/app.svelte";
  import type { McpState } from "./mcp.svelte";

  let { mcp }: { mcp: McpState } = $props();

  const app = getApp();
  /** Commands waiting for the go-ahead, and what to run then. */
  let confirm = $state<{ commands: string[]; run: () => Promise<void> } | null>(null);

  $effect(() => {
    void app.loadCount;
    void app.collection?.id;
    void mcp.loadTests();
  });

  async function guarded(paths: string[], run: () => Promise<void>) {
    const commands = await mcp.stdioCommands(paths);
    if (commands.length === 0) return run();
    confirm = { commands, run };
  }

  const anyRunning = $derived(Object.values(mcp.running).some(Boolean));
</script>

<div class="mcp-side" data-testid="mcp-sidebar">
  <div class="actions">
    <button class="button primary" data-testid="mcp-new" onclick={() => mcp.newConnection()}>New connection</button>
    <button
      class="button"
      data-testid="mcp-run-all"
      disabled={mcp.tests.length === 0 || anyRunning}
      onclick={() => guarded(mcp.tests, () => mcp.runAll())}>Run all</button
    >
  </div>

  {#if confirm}
    <div class="confirm" role="alertdialog" aria-labelledby="rt-mcp-confirm" data-testid="mcp-stdio-confirm">
      <p id="rt-mcp-confirm">These tests start local commands on this computer:</p>
      <ul>
        {#each confirm.commands as c (c)}<li class="mono">{c}</li>{/each}
      </ul>
      <p class="muted">Run them only if you trust this collection.</p>
      <div class="actions">
        <button class="button" onclick={() => (confirm = null)}>Cancel</button>
        <button
          class="button primary"
          data-testid="mcp-stdio-allow"
          onclick={async () => {
            const run = confirm?.run;
            confirm = null;
            mcp.allowStdio();
            await run?.();
          }}>Run</button
        >
      </div>
    </div>
  {/if}

  <h3>Saved tests</h3>
  {#if mcp.testsError}<p class="error" role="alert">{mcp.testsError}</p>{/if}
  {#if mcp.tests.length > 0 && Object.keys(mcp.testResults).length > 0}
    <p class="summary" data-testid="mcp-tests-summary" aria-live="polite">
      <span class="ok">{mcp.counts.passed} passed</span>
      <span class={mcp.counts.failed > 0 ? "bad" : "muted"}>{mcp.counts.failed} failed</span>
    </p>
  {/if}
  <ul class="tests" data-testid="mcp-tests" data-count={mcp.tests.length}>
    {#each mcp.tests as path (path)}
      {@const r = mcp.testResults[path]}
      {@const state = mcp.running[path] ? "running" : r ? (r.passed ? "passed" : "failed") : "none"}
      <li data-testid="mcp-test" data-path={path} data-state={state} class:current={mcp.testPath === path}>
        <span class="state {state}" aria-label={state}>{state === "passed" ? "✓" : state === "failed" ? "✗" : state === "running" ? "…" : "·"}</span>
        <button class="name" title={path} onclick={() => mcp.openTest(path)}>{path.replace(/^mcp\//, "").replace(/\.mcp\.yaml$/, "")}</button>
        <button class="link" data-testid="mcp-test-run" disabled={mcp.running[path]} onclick={() => guarded([path], () => mcp.runTest(path))}>Run</button>
        {#if r && !r.passed}
          <div class="why" data-testid="mcp-test-failure">
            {#if r.error}{r.error}{/if}
            {#each r.assertions.filter((a) => !a.passed) as a, i (i)}
              <div>{a.expression} {a.operator} {a.expected}: got {a.actual ?? a.error ?? "nothing"}</div>
            {/each}
            {#if !r.error && r.assertions.length === 0 && r.result?.is_error}The tool reported an error.{/if}
          </div>
        {/if}
      </li>
    {:else}
      <li class="muted empty">No saved tests yet. Call a tool and choose “Save as test”.</li>
    {/each}
  </ul>
</div>

<style>
  .mcp-side {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 8px;
    min-height: 0;
    overflow: auto;
  }
  .actions {
    display: flex;
    gap: 6px;
    justify-content: flex-end;
  }
  .actions:first-child {
    justify-content: flex-start;
  }
  h3 {
    margin: 4px 0 0;
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
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
  }
  .link:disabled {
    opacity: 0.5;
  }
  .tests {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .tests li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    padding: 3px 4px;
    border-radius: var(--rt-radius);
  }
  .tests li.current {
    background: var(--rt-bg-selected);
  }
  .name {
    flex: 1;
    min-width: 0;
    text-align: left;
    border: 0;
    background: none;
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    padding: 0;
  }
  .state {
    width: 1em;
    text-align: center;
  }
  .state.passed,
  .ok {
    color: var(--rt-ok);
  }
  .state.failed,
  .bad,
  .error {
    color: var(--rt-danger);
  }
  .why {
    width: 100%;
    padding-left: 1.5em;
    font-size: var(--rt-size-small);
    color: var(--rt-danger);
    word-break: break-word;
  }
  .summary {
    display: flex;
    gap: 10px;
    margin: 0;
  }
  .confirm {
    border: 1px solid var(--rt-warn);
    border-radius: var(--rt-radius);
    padding: 8px;
    background: var(--rt-bg-panel);
  }
  .confirm p,
  .confirm ul {
    margin: 0 0 6px;
  }
  .mono {
    font-family: var(--rt-font-mono);
    font-size: var(--rt-size-small);
    word-break: break-all;
  }
  .muted {
    color: var(--rt-fg-muted);
  }
  .empty {
    font-size: var(--rt-size-small);
  }
</style>
