<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The MCP client in the main area (wave 8, strand D): connection form (stdio command,
     arguments, environment and working folder, or Streamable HTTP URL and headers), the
     server's tools with descriptions and annotations, the selected tool's arguments, the
     call, its result, and "Save as test" with assertions. Opened from the sidebar view "MCP"
     and the palette ("Test an MCP server"). {{variables}} come from the active environment. -->
<script lang="ts">
  import { isUnary, newAssertion, OPERATORS } from "../scripts/edit";
  import { getApp } from "../state/app.svelte";
  import ArgsForm from "./ArgsForm.svelte";
  import type { McpState } from "./mcp.svelte";
  import { annotationLabels, toolLabel } from "./model";
  import ResultView from "./ResultView.svelte";
  import { fieldsOf } from "./schema";

  let { mcp }: { mcp: McpState } = $props();

  const app = getApp();
  const fields = $derived(mcp.tool ? fieldsOf(mcp.tool.input_schema) : null);
  const kind = $derived<"headers" | "env">(mcp.draft.transport === "http" ? "headers" : "env");
  const stdioInWeb = $derived(app.backend.kind === "web" && mcp.draft.transport === "stdio");

  function rows(kind: "headers" | "env") {
    return mcp.draft[kind];
  }
  function setRow(kind: "headers" | "env", i: number, at: 0 | 1, value: string) {
    const next = rows(kind).map((r): [string, string] => [r[0], r[1]]);
    next[i]![at] = value;
    mcp.draft[kind] = next;
  }
</script>

<section class="mcp" data-testid="mcp-panel" aria-labelledby="rt-mcp-title">
  <div class="bar">
    <h2 id="rt-mcp-title">MCP server{#if mcp.testPath}<span class="muted"> · {mcp.testPath}</span>{/if}</h2>
    <span class="spacer"></span>
    <button class="close" data-testid="mcp-close" aria-label="Close MCP client" onclick={() => mcp.close()}>×</button>
  </div>

  <form
    class="connect"
    data-testid="mcp-connect-form"
    onsubmit={(e) => {
      e.preventDefault();
      void mcp.connect();
    }}
  >
    <div class="row">
      <select aria-label="Transport" data-testid="mcp-transport" bind:value={mcp.draft.transport}>
        <option value="http">Streamable HTTP</option>
        <option value="stdio">stdio (local command)</option>
      </select>
      {#if mcp.draft.transport === "http"}
        <input class="grow" type="text" spellcheck="false" aria-label="Server URL" placeholder={"https://example.com/mcp or {{mcpUrl}}"} data-testid="mcp-url" bind:value={mcp.draft.url} />
      {:else}
        <input class="grow" type="text" spellcheck="false" aria-label="Command" placeholder="npx, uvx, ./server …" data-testid="mcp-command" bind:value={mcp.draft.command} />
      {/if}
      {#if mcp.connection}
        <button type="button" class="button" data-testid="mcp-disconnect" onclick={() => mcp.disconnect()}>Disconnect</button>
      {/if}
      <button type="submit" class="button primary" data-testid="mcp-connect" disabled={mcp.connecting || stdioInWeb}>
        {mcp.connecting ? "Connecting…" : mcp.connection ? "Reconnect" : "Connect"}
      </button>
    </div>
    {#if stdioInWeb}
      <p class="muted" data-testid="mcp-stdio-web">The web app cannot start local commands. Connect over HTTP, or use the desktop app or <code>rt mcp-test</code>.</p>
    {/if}
    <details class="more">
      <summary>{mcp.draft.transport === "http" ? `Headers (${mcp.draft.headers.length})` : `Arguments, environment, folder`}</summary>
      {#if mcp.draft.transport === "stdio"}
        <label class="stack">
          Arguments, one per line
          <textarea rows="3" spellcheck="false" data-testid="mcp-args-lines" bind:value={mcp.draft.args}></textarea>
        </label>
        <label class="stack">
          Working folder (relative to the collection)
          <input type="text" spellcheck="false" data-testid="mcp-cwd" bind:value={mcp.draft.cwd} />
        </label>
      {/if}
      <div class="pairs" data-testid="mcp-{kind}">
        {#each rows(kind) as row, i (i)}
          <div class="pair">
            <input type="text" spellcheck="false" aria-label="Name" placeholder="name" value={row[0]} oninput={(e) => setRow(kind, i, 0, e.currentTarget.value)} />
            <input type="text" spellcheck="false" aria-label="Value" placeholder={"value or {{secret}}"} value={row[1]} oninput={(e) => setRow(kind, i, 1, e.currentTarget.value)} />
            <button type="button" class="link" aria-label="Remove" onclick={() => (mcp.draft[kind] = rows(kind).filter((_, j) => j !== i))}>Remove</button>
          </div>
        {/each}
        <button type="button" class="link" data-testid="mcp-add-{kind}" onclick={() => (mcp.draft[kind] = [...rows(kind), ["", ""]])}>
          {kind === "headers" ? "Add header" : "Add environment variable"}
        </button>
        <span class="muted">Put secrets in environment variables and write <code>{"{{name}}"}</code>; saved tests keep only the reference.</span>
      </div>
    </details>
  </form>

  {#if mcp.error}<p class="error" role="alert" data-testid="mcp-error">{mcp.error}</p>{/if}

  {#if mcp.connection}
    <p class="info" data-testid="mcp-server-info">
      Connected to <strong>{mcp.connection.info.name}</strong> {mcp.connection.info.version}
      <span class="muted">· protocol {mcp.connection.info.protocol_version} · environment {app.environment ?? "none"}</span>
    </p>
    {#if mcp.connection.info.instructions}<p class="muted instructions">{mcp.connection.info.instructions}</p>{/if}
  {/if}

  <div class="work">
    {#if mcp.connection}
      <ul class="tools" role="listbox" aria-label="Tools" data-testid="mcp-tools" data-count={mcp.tools.length}>
        {#each mcp.tools as t (t.name)}
          <li
            role="option"
            tabindex="0"
            aria-selected={mcp.selected === t.name}
            data-testid="mcp-tool"
            data-name={t.name}
            onclick={() => mcp.select(t.name)}
            onkeydown={(e) => (e.key === "Enter" || e.key === " ") && (e.preventDefault(), mcp.select(t.name))}
          >
            <span class="tool-name">{toolLabel(t)}</span>
            {#each annotationLabels(t) as a (a)}<span class="tag">{a}</span>{/each}
            {#if t.description}<span class="muted desc">{t.description}</span>{/if}
          </li>
        {:else}
          <li class="muted">The server has no tools.</li>
        {/each}
      </ul>
    {/if}

    {#if mcp.selected && (mcp.connection || mcp.testPath)}
      <div class="call">
        <h3>{mcp.tool ? toolLabel(mcp.tool) : mcp.selected}</h3>
        <ArgsForm
          {fields}
          args={mcp.args}
          raw={mcp.raw}
          rawText={mcp.rawText}
          onargs={(a) => mcp.setArgs(a)}
          onraw={(text, parsed) => mcp.setRaw(text, parsed)}
          onmode={(raw) => (mcp.raw = raw)}
        />
        <div class="row">
          <button class="button primary" data-testid="mcp-call" disabled={!mcp.connection || mcp.calling} onclick={() => mcp.call()}>
            {mcp.calling ? "Calling…" : "Call tool"}
          </button>
          <button class="button" data-testid="mcp-save-test" disabled={mcp.saving !== null} onclick={() => mcp.startSave()}>
            {mcp.testPath ? "Update test…" : "Save as test…"}
          </button>
          {#if !mcp.connection}<span class="muted">Connect to call the tool.</span>{/if}
        </div>

        {#if mcp.saving}
          <form
            class="save"
            data-testid="mcp-save-form"
            onsubmit={(e) => {
              e.preventDefault();
              void mcp.save();
            }}
          >
            <label class="stack">
              Test name
              <input type="text" data-testid="mcp-test-name" bind:value={mcp.saving.name} />
            </label>
            <span class="muted">Assertions on <code>res.status</code> (200, 500 for a tool error), <code>res.body</code> (structured output, else the first text) and <code>res.responseTime</code>.</span>
            {#each mcp.saving.assertions as a, i (i)}
              <div class="assertion" data-testid="mcp-assertion">
                <input type="checkbox" aria-label="Enabled" bind:checked={a.enabled} />
                <input type="text" spellcheck="false" aria-label="Expression" bind:value={a.expression} />
                <select aria-label="Operator" bind:value={a.operator}>
                  {#each OPERATORS as op (op)}<option value={op}>{op}</option>{/each}
                </select>
                <input type="text" spellcheck="false" aria-label="Expected value" disabled={isUnary(a.operator)} bind:value={a.value} />
                <button type="button" class="link" onclick={() => mcp.saving && (mcp.saving.assertions = mcp.saving.assertions.filter((_, j) => j !== i))}>Remove</button>
              </div>
            {/each}
            <div class="row">
              <button type="button" class="link" data-testid="mcp-add-assertion" onclick={() => mcp.saving && (mcp.saving.assertions = [...mcp.saving.assertions, newAssertion()])}>Add assertion</button>
              <span class="spacer"></span>
              <button type="button" class="button" onclick={() => (mcp.saving = null)}>Cancel</button>
              {#if mcp.testPath}
                <button type="button" class="button" data-testid="mcp-save-copy" onclick={() => mcp.save(true)}>Save as new</button>
              {/if}
              <button type="submit" class="button primary" data-testid="mcp-save-confirm">Save test</button>
            </div>
            {#if mcp.saveError}<p class="error" role="alert" data-testid="mcp-save-error">{mcp.saveError}</p>{/if}
            <span class="muted">Staged like request edits; written to the collection folder on Save.</span>
          </form>
        {/if}

        {#if mcp.result}
          <ResultView result={mcp.result} />
        {/if}
      </div>
    {:else if !mcp.connection}
      <p class="muted hint" data-testid="mcp-hint">Connect to an MCP server to list and call its tools, then save calls as tests.</p>
    {/if}
  </div>
</section>

<style>
  .mcp {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
    padding: 0 12px var(--rt-gap);
    overflow: auto;
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
  h3 {
    margin: 0;
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
  .connect,
  .save {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .grow {
    flex: 1;
    min-width: 160px;
  }
  input[type="text"],
  select,
  textarea {
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg);
    padding: 3px 6px;
    min-width: 0;
  }
  textarea {
    font-family: var(--rt-font-mono);
  }
  .stack {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .more summary {
    cursor: pointer;
    color: var(--rt-fg-muted);
  }
  .more {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .pairs {
    display: flex;
    flex-direction: column;
    gap: 4px;
    align-items: flex-start;
    margin-top: 4px;
  }
  .pair,
  .assertion {
    display: flex;
    gap: 6px;
    align-items: center;
    width: 100%;
  }
  .pair input,
  .assertion input[type="text"] {
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
  .link {
    border: 0;
    background: none;
    color: var(--rt-accent);
    cursor: pointer;
    padding: 0;
  }
  .work {
    display: flex;
    gap: 12px;
    min-height: 0;
    align-items: flex-start;
  }
  .tools {
    list-style: none;
    margin: 0;
    padding: 0;
    width: 240px;
    flex-shrink: 0;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    max-height: 60vh;
    overflow: auto;
  }
  .tools li {
    padding: 6px 8px;
    border-bottom: 1px solid var(--rt-border);
    cursor: pointer;
    display: flex;
    flex-wrap: wrap;
    gap: 2px 6px;
    align-items: baseline;
  }
  .tools li:hover {
    background: var(--rt-bg-hover);
  }
  .tools li[aria-selected="true"] {
    background: var(--rt-bg-selected);
  }
  .tool-name {
    font-family: var(--rt-font-mono);
    font-weight: 600;
  }
  .tag {
    font-size: 11px;
    padding: 0 5px;
    border: 1px solid var(--rt-border);
    border-radius: 8px;
    color: var(--rt-fg-muted);
  }
  .desc {
    width: 100%;
    font-size: var(--rt-size-small);
  }
  .call {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .save {
    padding: 8px;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg-panel);
  }
  .info,
  .instructions {
    margin: 0;
  }
  .muted {
    color: var(--rt-fg-muted);
    margin: 0;
  }
  .error {
    color: var(--rt-danger);
    margin: 0;
  }
  .hint {
    padding: 16px 0;
  }
</style>
