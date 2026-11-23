<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Sidebar view "MCP" (wave 8, strand D; explained in wave 14). First visit: what MCP is and
     what happens here (McpIntro), with "Try the demo server", "Connect to a server…" and
     "Learn more". Then the guided first run (McpGuide) and the collection's saved MCP tests
     (*.mcp.yaml) to open, run one by one or all, with pass/fail of the last run. Tests that
     start a local command (stdio) run only after the user agreed once per collection. -->
<script lang="ts">
  import { getApp } from "../state/app.svelte";
  import { Banner, Button, EmptyState, Icon, IconButton, Section, Tooltip } from "../ui";
  import McpGuide from "./McpGuide.svelte";
  import McpIntro from "./McpIntro.svelte";
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
  /** First visit: nothing saved or connected, the client not open, the explanation not closed. */
  const firstVisit = $derived(mcp.tests.length === 0 && !mcp.connection && !mcp.open && mcp.introOpen);
  /** The guide once something is connected; finished, it stays until the next start. */
  const showGuide = $derived(!mcp.guideHidden && (mcp.guide.connect || (mcp.connection !== null && mcp.guideStep !== null)));
  const label = (path: string) => path.replace(/^mcp\//, "").replace(/\.mcp\.yaml$/, "");
</script>

<div class="mcp-side" data-testid="mcp-sidebar">
  {#if firstVisit}
    <McpIntro {mcp} />
  {:else}
    <div class="toolbar">
      <Tooltip text="Connect to the demo server built into RequestTailor">
        <Button size="sm" icon="play" data-testid="mcp-try-demo" onclick={() => mcp.tryDemo()}>Demo</Button>
      </Tooltip>
      <Tooltip text="Connect to any MCP server by command or URL">
        <Button size="sm" icon="add" data-testid="mcp-new" onclick={() => mcp.newConnection()}>Connect…</Button>
      </Tooltip>
      <span class="spacer"></span>
      <IconButton
        icon="help"
        size="sm"
        label={mcp.introOpen ? "Hide what MCP is" : "What is MCP?"}
        pressed={mcp.introOpen}
        data-testid="mcp-about"
        onclick={() => mcp.setIntroOpen(!mcp.introOpen)}
      />
    </div>
    {#if mcp.introOpen}
      <Banner tone="info" icon="mcp" title="What happens here" dismissible dismissLabel="Hide" ondismiss={() => mcp.setIntroOpen(false)}>
        AI assistants call tools on MCP servers. Here you test such servers like APIs and keep calls as tests;
        <code>rt mcp</code> is the other direction and lets agents use your collections.
        {#snippet actions()}
          {#if mcp.guideHidden}
            <Tooltip text="Walk through connect, call, save and run with the demo server">
              <Button size="sm" variant="ghost" data-testid="mcp-guide-show" onclick={() => (mcp.showGuide(), mcp.tryDemo())}>Start the guide</Button>
            </Tooltip>
          {/if}
        {/snippet}
      </Banner>
    {/if}
  {/if}

  {#if showGuide}<McpGuide {mcp} />{/if}

  {#if confirm}
    <Banner tone="warn" title="These tests start local commands">
      <ul class="commands" data-testid="mcp-stdio-confirm">
        {#each confirm.commands as c (c)}<li class="mono">{c}</li>{/each}
      </ul>
      Run them only if you trust this collection.
      {#snippet actions()}
        <Button size="sm" onclick={() => (confirm = null)}>Cancel</Button>
        <Button
          size="sm"
          variant="primary"
          data-testid="mcp-stdio-allow"
          onclick={async () => {
            const run = confirm?.run;
            confirm = null;
            mcp.allowStdio();
            await run?.();
          }}>Run</Button
        >
      {/snippet}
    </Banner>
  {/if}

  {#if !firstVisit}
    <Section title="Saved tests" count={mcp.tests.length}>
      {#snippet actions()}
        <Tooltip text={mcp.tests.length === 0 ? "Save a tool call as a test first" : "Run every saved MCP test of this collection"}>
          <Button
            size="sm"
            variant={mcp.guideStep === "run" ? "primary" : "secondary"}
            icon="play"
            data-testid="mcp-run-all"
            disabled={mcp.tests.length === 0 || anyRunning}
            onclick={() => guarded(mcp.tests, () => mcp.runAll())}>Run all</Button
          >
        </Tooltip>
      {/snippet}
      {#if mcp.testsError}<Banner tone="danger">{mcp.testsError}</Banner>{/if}
      {#if mcp.tests.length > 0 && Object.keys(mcp.testResults).length > 0}
        <p class="summary" data-testid="mcp-tests-summary" aria-live="polite">
          <span class="ok">{mcp.counts.passed} passed</span>
          <span class={mcp.counts.failed > 0 ? "bad" : "muted"}>{mcp.counts.failed} failed</span>
        </p>
      {/if}
      {#if mcp.tests.length === 0}
        <div data-testid="mcp-tests" data-count="0">
          <EmptyState size="sm" title="No saved tests yet" text="Call a tool and choose “Save as test…”. Tests are files next to your collection." />
        </div>
      {:else}
        <ul class="tests" data-testid="mcp-tests" data-count={mcp.tests.length}>
          {#each mcp.tests as path (path)}
            {@const r = mcp.testResults[path]}
            {@const state = mcp.running[path] ? "running" : r ? (r.passed ? "passed" : "failed") : "none"}
            <li data-testid="mcp-test" data-path={path} data-state={state} class:current={mcp.testPath === path}>
              <div class="row">
                <span class="state {state}" role="img" aria-label={state}>
                  {#if state === "passed"}<Icon name="success" size="sm" />{:else if state === "failed"}<Icon name="error" size="sm" />{:else if state === "running"}<Icon name="sync" size="sm" />{:else}<span class="dot"></span>{/if}
                </span>
                <Tooltip text="Open {path}">
                  <Button variant="ghost" size="sm" block class="name" onclick={() => mcp.openTest(path)}>{label(path)}</Button>
                </Tooltip>
                <IconButton icon="play" size="sm" label="Run this test" data-testid="mcp-test-run" disabled={mcp.running[path]} onclick={() => guarded([path], () => mcp.runTest(path))} />
              </div>
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
          {/each}
        </ul>
      {/if}
    </Section>
  {/if}
</div>

<style>
  .mcp-side {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-6);
    padding: var(--rt-space-4) var(--rt-space-4) var(--rt-space-6);
    min-height: 0;
    overflow: auto;
  }
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-3);
  }
  .spacer {
    flex: 1;
  }
  .tests {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-1);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .tests li {
    border-radius: var(--rt-radius-md);
  }
  .tests li.current {
    background: var(--rt-bg-selected);
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--rt-space-2);
    min-height: var(--rt-control-md);
    padding-left: var(--rt-space-2);
  }
  .row > :global(.rt-tooltip-anchor) {
    flex: 1;
    min-width: 0;
  }
  .row :global(.name) {
    justify-content: flex-start;
    min-width: 0;
  }
  .state {
    display: grid;
    place-items: center;
    flex: none;
    width: 16px;
    color: var(--rt-fg-subtle);
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: var(--rt-radius-pill);
    background: var(--rt-border-strong);
  }
  .state.passed,
  .ok {
    color: var(--rt-ok-fg);
  }
  .state.failed,
  .bad {
    color: var(--rt-danger-fg);
  }
  .state.running {
    color: var(--rt-accent);
  }
  .why {
    padding: 0 var(--rt-space-3) var(--rt-space-3) calc(16px + var(--rt-space-4));
    font: var(--rt-font-small);
    color: var(--rt-danger-fg);
    word-break: break-word;
  }
  .summary {
    display: flex;
    gap: var(--rt-space-5);
    margin: 0;
    font: var(--rt-font-small);
  }
  .commands {
    margin: 0 0 var(--rt-space-2);
    padding-left: var(--rt-space-5);
  }
  .mono {
    font: var(--rt-font-small);
    font-family: var(--rt-font-mono);
    word-break: break-all;
  }
  .muted {
    color: var(--rt-fg-muted);
  }
</style>
