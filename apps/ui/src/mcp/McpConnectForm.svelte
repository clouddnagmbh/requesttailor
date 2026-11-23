<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The connection form of the MCP client (wave 8; on the primitives since wave 14):
     Streamable HTTP URL and headers, or a stdio command with arguments, environment and
     working folder. {{variables}} come from the active environment. -->
<script lang="ts">
  import { getApp } from "../state/app.svelte";
  import { Button, IconButton, Select, TextArea, TextField, Tooltip } from "../ui";
  import type { McpState } from "./mcp.svelte";

  let { mcp }: { mcp: McpState } = $props();

  const app = getApp();
  const kind = $derived<"headers" | "env">(mcp.draft.transport === "http" ? "headers" : "env");
  // The web shell cannot start commands; it says so instead of offering to.
  const stdioInWeb = $derived(app.backend.kind === "web" && mcp.draft.transport === "stdio");
  let target = $state<HTMLInputElement>();

  export function focus() {
    target?.focus();
  }

  function rows(k: "headers" | "env") {
    return mcp.draft[k];
  }
  function setRow(k: "headers" | "env", i: number, at: 0 | 1, value: string) {
    const next = rows(k).map((r): [string, string] => [r[0], r[1]]);
    next[i]![at] = value;
    mcp.draft[k] = next;
  }
</script>

<form
  class="connect"
  data-testid="mcp-connect-form"
  onsubmit={(e) => {
    e.preventDefault();
    void mcp.connect();
  }}
>
  <div class="row">
    <div class="transport">
      <Select
        aria-label="Transport"
        data-testid="mcp-transport"
        bind:value={mcp.draft.transport}
        options={[
          { value: "http", label: "Streamable HTTP" },
          { value: "stdio", label: "stdio (local command)" },
        ]}
      />
    </div>
    <div class="target">
      {#if mcp.draft.transport === "http"}
        <TextField
          bind:element={target}
          mono
          spellcheck="false"
          aria-label="Server URL"
          placeholder={"https://example.com/mcp or {{mcpUrl}}"}
          data-testid="mcp-url"
          bind:value={mcp.draft.url}
        />
      {:else}
        <TextField
          bind:element={target}
          mono
          spellcheck="false"
          aria-label="Command"
          placeholder="npx, uvx, ./server …"
          data-testid="mcp-command"
          bind:value={mcp.draft.command}
        />
      {/if}
    </div>
    {#if mcp.connection}
      <Tooltip text="Close the connection (stdio servers are stopped)">
        <Button data-testid="mcp-disconnect" onclick={() => mcp.disconnect()}>Disconnect</Button>
      </Tooltip>
    {/if}
    <Tooltip text={mcp.connection ? "Connect again, e.g. after changing the server" : "Start or reach the server and list what it offers"}>
      <Button type="submit" variant={mcp.connection ? "secondary" : "primary"} data-testid="mcp-connect" disabled={mcp.connecting || stdioInWeb} loading={mcp.connecting}>
        {mcp.connection ? "Reconnect" : "Connect"}
      </Button>
    </Tooltip>
  </div>
  {#if stdioInWeb}
    <p class="muted" data-testid="mcp-stdio-web">The web app cannot start local commands. Connect over HTTP, or use the desktop app or <code>rt mcp-test</code>.</p>
  {/if}
  <details class="more">
    <summary>{mcp.draft.transport === "http" ? `Headers (${mcp.draft.headers.length})` : `Arguments, environment, folder`}</summary>
    <div class="more-body">
      {#if mcp.draft.transport === "stdio"}
        <TextArea label="Arguments, one per line" rows={3} mono spellcheck="false" data-testid="mcp-args-lines" bind:value={mcp.draft.args} />
        <TextField label="Working folder (relative to the collection)" mono spellcheck="false" data-testid="mcp-cwd" bind:value={mcp.draft.cwd} />
      {/if}
      <div class="pairs" data-testid="mcp-{kind}">
        {#each rows(kind) as row, i (i)}
          <div class="pair">
            <TextField size="sm" mono spellcheck="false" aria-label="Name" placeholder="name" value={row[0]} oninput={(e) => setRow(kind, i, 0, e.currentTarget.value)} />
            <TextField size="sm" mono spellcheck="false" aria-label="Value" placeholder={"value or {{secret}}"} value={row[1]} oninput={(e) => setRow(kind, i, 1, e.currentTarget.value)} />
            <IconButton size="sm" icon="delete" label="Remove" onclick={() => (mcp.draft[kind] = rows(kind).filter((_, j) => j !== i))} />
          </div>
        {/each}
        <div>
          <Button size="sm" variant="ghost" icon="add" data-testid="mcp-add-{kind}" onclick={() => (mcp.draft[kind] = [...rows(kind), ["", ""]])}>
            {kind === "headers" ? "Add header" : "Add environment variable"}
          </Button>
        </div>
        <p class="muted">Put secrets in environment variables and write <code>{"{{name}}"}</code>; saved tests keep only the reference.</p>
      </div>
    </div>
  </details>
</form>

<style>
  .connect {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-3);
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-3);
  }
  .transport {
    flex: none;
    width: 190px;
  }
  .target {
    flex: 1;
    min-width: 200px;
  }
  .more summary {
    cursor: pointer;
    font: var(--rt-font-small);
    color: var(--rt-fg-muted);
  }
  .more-body {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-3);
    padding-top: var(--rt-space-3);
  }
  .pairs {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-2);
  }
  .pair {
    display: grid;
    grid-template-columns: 1fr 1fr auto;
    gap: var(--rt-space-2);
    align-items: center;
  }
  .muted {
    margin: 0;
    font: var(--rt-font-small);
    color: var(--rt-fg-muted);
  }
</style>
