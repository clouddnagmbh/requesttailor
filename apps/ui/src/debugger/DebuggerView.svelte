<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The script debugger (wave 19, strand Y): pick the script of the open request, click the
     gutter for breakpoints, run it, step, read variables and ask questions in the paused scope.
     Values shown are masked by the core; nothing is sent from here. -->
<script lang="ts">
  import { onDestroy } from "svelte";

  import type { Breakpoint, DebugCommand, DebugState, ScriptPhase } from "../commands/types";
  import type { ToolProps } from "../shell/tool-host.svelte";
  import { Badge, Banner, Button, EmptyState, Select, TextField } from "../ui";
  import { PHASES, isOver, isPaused, linesOf, needsResponse, showValue, statusLine, toggleBreakpoint } from "./model";

  let { app }: ToolProps = $props();

  const path = $derived(app.active);
  let phase = $state<ScriptPhase>("before-request");
  let code = $state<string | null>(null);
  let breakpoints = $state<Breakpoint[]>([]);
  let session = $state<DebugState | null>(null);
  let expression = $state("");
  let error = $state<string | null>(null);
  let busy = $state(false);

  const responseId = $derived(path ? (app.results[path]?.response_id ?? null) : null);
  const lines = $derived(code === null ? [] : linesOf(code));
  const pause = $derived(session?.pause ?? null);
  const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

  $effect(() => {
    const p = path;
    const ph = phase;
    code = null;
    if (!p) return;
    void app.backend
      .getRequest(p)
      .then((spec) => {
        if (p === path && ph === phase) code = spec.scripts.find((s) => s.phase === ph)?.code ?? "";
      })
      .catch((e) => (error = message(e)));
  });

  async function guarded(f: () => Promise<DebugState | void>) {
    if (busy) return;
    busy = true;
    error = null;
    try {
      const next = await f();
      if (next) session = next;
    } catch (e) {
      error = message(e);
    } finally {
      busy = false;
    }
  }

  async function begin() {
    if (!path) return;
    await end();
    const target = {
      request_path: path,
      phase,
      ...(app.environment ? { environment: app.environment } : {}),
      ...(needsResponse(phase) && responseId !== null ? { response_id: responseId } : {}),
      breakpoints,
      stop_on_entry: breakpoints.length === 0,
    };
    await guarded(() => app.backend.debugger.start(target));
  }

  async function end() {
    const id = session?.session;
    session = null;
    if (id !== undefined) await app.backend.debugger.stop(id).catch(() => {});
  }

  function send(command: DebugCommand) {
    const id = session?.session;
    if (id === undefined) return Promise.resolve();
    return guarded(() => app.backend.debugger.command(id, command));
  }

  function toggle(line: number) {
    breakpoints = toggleBreakpoint(breakpoints, line);
    if (isPaused(session)) void send({ kind: "set-breakpoints", breakpoints });
  }

  async function ask() {
    const text = expression.trim();
    if (!text) return;
    await send({ kind: "evaluate", expression: text });
  }

  onDestroy(() => void end());
</script>

<section class="debugger" data-testid="debugger-panel" aria-label="Script debugger">
  <header>
    <h2>Script debugger</h2>
    <p class="note">Runs one script of the open request in the sandbox and stops where you ask. Nothing is sent. Secret values show as ***.</p>
  </header>

  {#if !path}
    <EmptyState icon="code" title="No request open" text="Open a request that has a script, then come back." />
  {:else}
    <div class="bar">
      <Select
        label="Script"
        bind:value={phase}
        disabled={session !== null && !isOver(session)}
        options={PHASES.map((p) => ({ value: p, label: p }))}
        data-testid="debugger-phase"
      />
      <Button variant="primary" icon="play" loading={busy} disabled={code === null || code.trim() === ""} onclick={begin} data-testid="debugger-start">
        {session && !isOver(session) ? "Restart" : "Debug"}
      </Button>
      {#if session && !isOver(session)}
        <Button onclick={() => send({ kind: "abort" })} data-testid="debugger-abort">Abort</Button>
      {/if}
      <Badge value={statusLine(session)} data-testid="debugger-status" />
    </div>
    {#if needsResponse(phase) && responseId === null}
      <Banner tone="info" title="Send the request once first">An after-response or tests script reads the response.</Banner>
    {/if}
    {#if error}
      <div data-testid="debugger-error"><Banner tone="danger" title="Debugger">{error}</Banner></div>
    {/if}

    {#if code === null}
      <p class="note">Reading the script.</p>
    {:else if code.trim() === ""}
      <EmptyState icon="code" title="No script" text={`This request has no ${phase} script.`} />
    {:else}
      <div class="body">
        <ol class="source" data-testid="debugger-source">
          {#each lines as text, i (i)}
            {@const n = i + 1}
            <li class:current={pause?.line === n} data-line={n} data-testid="debugger-line">
              <button
                class="gutter"
                class:on={breakpoints.some((b) => b.line === n)}
                aria-label={`Breakpoint at line ${n}`}
                aria-pressed={breakpoints.some((b) => b.line === n)}
                onclick={() => toggle(n)}
                data-testid="debugger-gutter"
              >{n}</button>
              <code>{text}</code>
            </li>
          {/each}
        </ol>

        <div class="side">
          <div class="steps">
            <Button disabled={!isPaused(session) || busy} onclick={() => send({ kind: "continue" })} data-testid="debugger-continue">Continue</Button>
            <Button disabled={!isPaused(session) || busy} onclick={() => send({ kind: "step-over" })} data-testid="debugger-step-over">Step over</Button>
            <Button disabled={!isPaused(session) || busy} onclick={() => send({ kind: "step-into" })} data-testid="debugger-step-into">Step into</Button>
            <Button disabled={!isPaused(session) || busy} onclick={() => send({ kind: "step-out" })} data-testid="debugger-step-out">Step out</Button>
          </div>

          {#if pause}
            <h3>Variables</h3>
            <ul class="vars" data-testid="debugger-vars">
              {#each pause.scopes as scope (scope.name)}
                {#each scope.values as v (scope.name + v.name)}
                  <li data-testid="debugger-var"><span class="scope">{scope.name}</span> <code>{showValue(v)}</code> <span class="kind">{v.kind}</span></li>
                {/each}
              {/each}
            </ul>
            <h3>Call stack</h3>
            <ul class="vars">
              {#each pause.frames as f, i (i)}<li><code>{f.function}</code> line {f.line}</li>{/each}
            </ul>
            <form
              onsubmit={(e) => {
                e.preventDefault();
                void ask();
              }}
            >
              <TextField label="Expression" bind:value={expression} mono data-testid="debugger-expression" />
            </form>
            {#if pause.evaluated}
              <p class="answer" data-testid="debugger-answer"><code>{showValue(pause.evaluated)}</code></p>
            {/if}
          {/if}

          {#if session?.output?.length}
            <h3>Output</h3>
            <pre class="out" data-testid="debugger-output">{session.output.join("\n")}</pre>
          {/if}
        </div>
      </div>
    {/if}
  {/if}
</section>

<style>
  .debugger {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-5);
    flex: 1;
    min-height: 0;
    overflow: auto;
    box-sizing: border-box;
    padding: var(--rt-space-6);
  }
  h2 {
    margin: 0;
    font: var(--rt-font-title);
  }
  h3 {
    margin: var(--rt-space-4) 0 var(--rt-space-2);
    font: var(--rt-font-heading);
  }
  .note,
  .kind,
  .scope {
    margin: 0;
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--rt-space-4);
  }
  .body {
    display: flex;
    flex-wrap: wrap;
    gap: var(--rt-space-6);
    min-width: 0;
  }
  .source {
    list-style: none;
    margin: 0;
    padding: 0;
    flex: 2 1 360px;
    min-width: 0;
    font-family: var(--rt-font-mono);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-md);
    overflow: auto;
  }
  .source li {
    display: flex;
    gap: var(--rt-space-3);
    white-space: pre;
  }
  .source li.current {
    background: var(--rt-warn-bg);
  }
  .gutter {
    all: unset;
    cursor: pointer;
    min-width: 3ch;
    padding: 0 var(--rt-space-3);
    text-align: right;
    color: var(--rt-fg-muted);
  }
  .gutter.on {
    background: var(--rt-danger-bg);
    color: var(--rt-danger-fg);
  }
  .gutter:focus-visible {
    outline: var(--rt-focus-ring);
  }
  .side {
    flex: 1 1 260px;
    min-width: 0;
  }
  .steps {
    display: flex;
    flex-wrap: wrap;
    gap: var(--rt-space-3);
  }
  .vars {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-2);
    overflow-wrap: anywhere;
  }
  .out {
    margin: 0;
    font-family: var(--rt-font-mono);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .answer {
    overflow-wrap: anywhere;
  }
</style>
