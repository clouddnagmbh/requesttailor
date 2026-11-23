<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The MCP client in the main area (wave 8, strand D; wave 14 on the primitives). Not
     connected: what to do first ("Try the demo server", "Connect to RequestTailor itself"),
     the connection form and, on desktop, the demo server on a local port. Connected: the
     server, its tools (form, call, result, "Save as test" with assertions), resources and
     prompts in tabs. The guided first run shows its next step as a banner. Opened from the
     sidebar view "MCP" and the palette ("Test an MCP server"). -->
<script lang="ts">
  import { MCP_DEMO_URL } from "../commands/backend";
  import { isUnary, newAssertion, OPERATORS } from "../scripts/edit";
  import { getApp } from "../state/app.svelte";
  import { Badge, Banner, Button, Card, EmptyState, Icon, IconButton, Tabs, Tooltip } from "../ui";
  import ArgsForm from "./ArgsForm.svelte";
  import { GUIDE_STEPS, GUIDE_TOOL } from "./guide";
  import McpConnectForm from "./McpConnectForm.svelte";
  import McpDiagram from "./McpDiagram.svelte";
  import McpPrompts from "./McpPrompts.svelte";
  import McpResources from "./McpResources.svelte";
  import type { McpState } from "./mcp.svelte";
  import { annotationLabels, configOf, LEARN_MORE_URL, serverTitle, toolLabel } from "./model";
  import ResultView from "./ResultView.svelte";
  import { fieldsOf } from "./schema";

  let { mcp }: { mcp: McpState } = $props();

  const app = getApp();
  const fields = $derived(mcp.tool ? fieldsOf(mcp.tool.input_schema) : null);
  const title = $derived(serverTitle(configOf(mcp.draft), MCP_DEMO_URL, mcp.localDemoUrl));
  const step = $derived(GUIDE_STEPS.find((s) => s.id === mcp.guideStep) ?? null);
  /** The coachmark only while the guide is running (after the first connect to the demo). */
  const coach = $derived(step !== null && mcp.guide.connect ? step : null);
  let form = $state<ReturnType<typeof McpConnectForm>>();

  $effect(() => {
    void mcp.refreshLocalDemo();
  });
  // "Connect to a server…" puts the cursor into the URL or command field.
  $effect(() => {
    if (mcp.focusForm > 0) form?.focus();
  });

  async function copy(text: string) {
    await navigator.clipboard?.writeText(text).catch(() => {});
  }
</script>

<section class="mcp" data-testid="mcp-panel" aria-labelledby="rt-mcp-title">
  <header class="bar">
    <span class="glyph"><Icon name="mcp" size="sm" /></span>
    <h2 id="rt-mcp-title">MCP</h2>
    {#if mcp.connection || mcp.testPath || mcp.draft.url || mcp.draft.command}
      <span class="sep">/</span>
      <span class="server" data-testid="mcp-server-title">{title}</span>
    {/if}
    {#if mcp.testPath}<Badge value={mcp.testPath} />{/if}
    <span class="spacer"></span>
    <IconButton icon="close" label="Close MCP client" data-testid="mcp-close" onclick={() => mcp.close()} />
  </header>

  <div class="body">
    {#if coach}
      <Banner tone="info" icon="spark" title="Next: {coach.title}" dismissible dismissLabel="Hide the guide" ondismiss={() => mcp.hideGuide()}>
        <span data-testid="mcp-coach" data-step={coach.id}>{coach.hint}</span>
      </Banner>
    {/if}
    {#if mcp.selfHint}
      <Banner tone={mcp.connection ? "info" : "warn"} icon="mcp" title="RequestTailor itself (rt mcp)" dismissible ondismiss={() => (mcp.selfHint = null)}>
        <span data-testid="mcp-self-hint">{mcp.selfHint}</span>
      </Banner>
    {/if}

    {#if !mcp.connection && !mcp.testPath && !mcp.connecting}
      <EmptyState icon="mcp" title="Test an MCP server" text="Connect to a server, see its tools, resources and prompts, call a tool and keep the call as a test.">
        {#snippet actions()}
          <Tooltip text="A demo server built into RequestTailor: fake data, offline, nothing to install">
            <Button variant="primary" icon="play" data-testid="mcp-panel-try-demo" onclick={() => mcp.tryDemo()}>Try the demo server</Button>
          </Tooltip>
          <Tooltip text="See your collection the way AI agents see it through rt mcp">
            <Button icon="mcp" data-testid="mcp-panel-try-self" onclick={() => mcp.trySelf()}>Connect to RequestTailor itself</Button>
          </Tooltip>
        {/snippet}
      </EmptyState>
      <div class="directions">
        <Card padding="sm" title="Test servers" description="RequestTailor is the client: it calls a server's tools and checks the answers, like an API test.">
          <McpDiagram direction="test" />
        </Card>
        <Card padding="sm" title="Serve your collection" description="rt mcp turns your requests into tools for AI agents, with read/write rules, approvals and an audit log.">
          <McpDiagram direction="agents" />
        </Card>
      </div>
    {/if}

    <Card padding="sm" title={mcp.connection ? undefined : "Connect to a server"} description={mcp.connection ? undefined : "By URL (Streamable HTTP) or by the command that starts it (stdio). {{variables}} come from the active environment."}>
      <McpConnectForm bind:this={form} {mcp} />
    </Card>

    {#if mcp.error}<Banner tone="danger" title="That did not work"><span data-testid="mcp-error">{mcp.error}</span></Banner>{/if}

    {#if !mcp.connection && mcp.api.localDemo}
      <Card padding="sm" icon="service" title="Demo server on a local port" description="The same demo over real HTTP on 127.0.0.1, for other MCP clients. It stops when you stop it or quit the app.">
        {#snippet actions()}
          {#if mcp.localDemoUrl}
            <Tooltip text="Stop the local demo server">
              <Button size="sm" data-testid="mcp-local-demo-stop" onclick={() => mcp.stopLocalDemo()}>Stop</Button>
            </Tooltip>
          {:else}
            <Tooltip text="Start the demo server on 127.0.0.1 with a free port">
              <Button size="sm" icon="play" data-testid="mcp-local-demo-start" onclick={() => mcp.startLocalDemo()}>Start</Button>
            </Tooltip>
          {/if}
        {/snippet}
        {#if mcp.localDemoUrl}
          <div class="local">
            <code data-testid="mcp-local-demo-url">{mcp.localDemoUrl}</code>
            <IconButton icon="copy" size="sm" label="Copy the URL" onclick={() => mcp.localDemoUrl && copy(mcp.localDemoUrl)} />
            <span class="spacer"></span>
            <Tooltip text="Connect this client to it over HTTP">
              <Button size="sm" variant="primary" data-testid="mcp-local-demo-connect" onclick={() => mcp.localDemoUrl && mcp.connectTo({ transport: "http", url: mcp.localDemoUrl, headers: [] })}>Connect</Button>
            </Tooltip>
          </div>
        {/if}
      </Card>
    {/if}

    {#if mcp.connection}
      <div class="info" data-testid="mcp-server-info">
        <span>Connected to <strong>{mcp.connection.info.name}</strong> {mcp.connection.info.version}</span>
        <Badge value="MCP {mcp.connection.info.protocol_version}" />
        <span class="muted">environment {app.environment ?? "none"}</span>
      </div>
      {#if mcp.connection.info.instructions}<p class="muted instructions">{mcp.connection.info.instructions}</p>{/if}

      <Tabs
        label="What the server offers"
        idPrefix="rt-mcp"
        bind:value={mcp.tab}
        items={[
          { id: "tools", label: "Tools", icon: "zap", count: mcp.tools.length },
          { id: "resources", label: "Resources", icon: "file-text", count: mcp.resources.length },
          { id: "prompts", label: "Prompts", icon: "spark", count: mcp.prompts.length },
        ]}
      />
    {/if}

    {#if mcp.connection && mcp.tab === "resources"}
      <div role="tabpanel" id="rt-mcp-panel-resources" aria-labelledby="rt-mcp-tab-resources"><McpResources {mcp} /></div>
    {:else if mcp.connection && mcp.tab === "prompts"}
      <div role="tabpanel" id="rt-mcp-panel-prompts" aria-labelledby="rt-mcp-tab-prompts"><McpPrompts {mcp} /></div>
    {:else if mcp.connection || mcp.testPath}
      <div class="work" role={mcp.connection ? "tabpanel" : undefined} id="rt-mcp-panel-tools" aria-labelledby={mcp.connection ? "rt-mcp-tab-tools" : undefined}>
        {#if mcp.connection}
          <ul class="tools" role="listbox" aria-label="Tools" data-testid="mcp-tools" data-count={mcp.tools.length}>
            {#each mcp.tools as t (t.name)}
              <li
                role="option"
                tabindex="0"
                aria-selected={mcp.selected === t.name}
                class:coach={coach?.id === "call" && t.name === GUIDE_TOOL && mcp.selected !== GUIDE_TOOL}
                data-testid="mcp-tool"
                data-name={t.name}
                onclick={() => mcp.select(t.name)}
                onkeydown={(e) => (e.key === "Enter" || e.key === " ") && (e.preventDefault(), mcp.select(t.name))}
              >
                <span class="tool-name">{toolLabel(t)}</span>
                {#each annotationLabels(t) as a (a)}<Badge value={a} />{/each}
                {#if t.description}<span class="desc">{t.description}</span>{/if}
              </li>
            {:else}
              <li class="muted">The server has no tools.</li>
            {/each}
          </ul>
        {/if}

        {#if mcp.selected}
          <div class="call">
            <h3>{mcp.tool ? toolLabel(mcp.tool) : mcp.selected}</h3>
            {#if mcp.tool?.description}<p class="muted">{mcp.tool.description}</p>{/if}
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
              <Tooltip text="Send tools/call with these arguments">
                <Button variant={coach?.id === "save" ? "secondary" : "primary"} icon="send" data-testid="mcp-call" disabled={!mcp.connection} loading={mcp.calling} onclick={() => mcp.call()}>Call tool</Button>
              </Tooltip>
              <Tooltip text="Keep this call as a test file (*.mcp.yaml) with checks on the result">
                <Button variant={coach?.id === "save" ? "primary" : "secondary"} data-testid="mcp-save-test" disabled={mcp.saving !== null} onclick={() => mcp.startSave()}>
                  {mcp.testPath ? "Update test…" : "Save as test…"}
                </Button>
              </Tooltip>
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
                  <span class="label">Test name</span>
                  <input type="text" class="rt-input" data-testid="mcp-test-name" bind:value={mcp.saving.name} />
                </label>
                <span class="muted small">Checks on <code>res.status</code> (200, 500 for a tool error), <code>res.body</code> (structured output, else the first text) and <code>res.responseTime</code>.</span>
                {#each mcp.saving.assertions as a, i (i)}
                  <div class="assertion" data-testid="mcp-assertion">
                    <input type="checkbox" aria-label="Enabled" bind:checked={a.enabled} />
                    <input type="text" class="rt-input" spellcheck="false" aria-label="Expression" bind:value={a.expression} />
                    <select class="rt-input" aria-label="Operator" bind:value={a.operator}>
                      {#each OPERATORS as op (op)}<option value={op}>{op}</option>{/each}
                    </select>
                    <input type="text" class="rt-input" spellcheck="false" aria-label="Expected value" disabled={isUnary(a.operator)} bind:value={a.value} />
                    <IconButton icon="delete" size="sm" label="Remove this check" onclick={() => mcp.saving && (mcp.saving.assertions = mcp.saving.assertions.filter((_, j) => j !== i))} />
                  </div>
                {/each}
                <div class="row">
                  <Button size="sm" variant="ghost" icon="add" data-testid="mcp-add-assertion" onclick={() => mcp.saving && (mcp.saving.assertions = [...mcp.saving.assertions, newAssertion()])}>Add check</Button>
                  <span class="spacer"></span>
                  <Button onclick={() => (mcp.saving = null)}>Cancel</Button>
                  {#if mcp.testPath}
                    <Button data-testid="mcp-save-copy" onclick={() => mcp.save(true)}>Save as new</Button>
                  {/if}
                  <Button type="submit" variant="primary" data-testid="mcp-save-confirm">Save test</Button>
                </div>
                {#if mcp.saveError}<p class="error" role="alert" data-testid="mcp-save-error">{mcp.saveError}</p>{/if}
                <span class="muted small">Staged like request edits; written to the collection folder on Save.</span>
              </form>
            {/if}

            {#if mcp.result}
              <ResultView result={mcp.result} />
            {/if}
          </div>
        {/if}
      </div>
    {:else if !mcp.connecting}
      <p class="muted hint" data-testid="mcp-hint">
        Connect to an MCP server to list and call its tools, then save calls as tests.
        <a href={LEARN_MORE_URL} target="_blank" rel="noopener noreferrer">Learn more</a>
      </p>
    {/if}
  </div>
</section>

<style>
  .mcp {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: var(--rt-space-3);
    min-height: var(--rt-bar-height);
    padding: 0 var(--rt-space-4) 0 var(--rt-space-6);
    border-bottom: 1px solid var(--rt-border);
  }
  .glyph {
    display: grid;
    place-items: center;
    width: var(--rt-control-sm);
    height: var(--rt-control-sm);
    border-radius: var(--rt-radius-sm);
    background: var(--rt-accent-subtle);
    color: var(--rt-accent-subtle-fg);
  }
  h2 {
    margin: 0;
    font: var(--rt-font-title);
  }
  .sep {
    color: var(--rt-fg-subtle);
  }
  .server {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--rt-fg-muted);
  }
  .spacer {
    flex: 1;
  }
  .body {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-5);
    padding: var(--rt-space-5) var(--rt-space-6) var(--rt-space-7);
    overflow: auto;
  }
  .body > :global(*) {
    flex: none;
  }
  .body > :global(.rt-empty) {
    padding-bottom: var(--rt-space-4);
  }
  .directions {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
    gap: var(--rt-space-4);
  }
  .local {
    display: flex;
    align-items: center;
    gap: var(--rt-space-3);
  }
  .local code {
    font: var(--rt-font-small);
    font-family: var(--rt-font-mono);
  }
  .info {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-3);
  }
  .instructions {
    margin-top: calc(-1 * var(--rt-space-3));
  }
  .work {
    display: flex;
    gap: var(--rt-space-5);
    min-height: 0;
    align-items: flex-start;
  }
  .tools {
    flex: none;
    width: 260px;
    max-height: 65vh;
    margin: 0;
    padding: var(--rt-space-2);
    overflow: auto;
    list-style: none;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-lg);
    background: var(--rt-surface-2);
  }
  .tools li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-1) var(--rt-space-2);
    padding: var(--rt-space-3) var(--rt-space-4);
    border-radius: var(--rt-radius-md);
    cursor: pointer;
  }
  .tools li:hover {
    background: var(--rt-bg-hover);
  }
  .tools li[aria-selected="true"] {
    background: var(--rt-accent-subtle);
    color: var(--rt-accent-subtle-fg);
  }
  .tools li.coach {
    box-shadow: inset 0 0 0 1px var(--rt-accent);
  }
  .tool-name {
    font-family: var(--rt-font-mono);
    font-weight: var(--rt-weight-semibold);
  }
  .desc {
    width: 100%;
    font: var(--rt-font-small);
    color: var(--rt-fg-muted);
  }
  .tools li[aria-selected="true"] .desc {
    color: inherit;
    opacity: 0.85;
  }
  .call {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-4);
  }
  h3 {
    margin: 0;
    font: var(--rt-font-title);
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-3);
  }
  .save {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-3);
    padding: var(--rt-space-5);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-lg);
    background: var(--rt-surface-2);
    box-shadow: var(--rt-elev-1);
  }
  .stack {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-2);
  }
  .label {
    font: var(--rt-font-small);
    font-weight: var(--rt-weight-medium);
    color: var(--rt-fg-muted);
  }
  .rt-input {
    min-width: 0;
    height: var(--rt-control-md);
    padding: 0 var(--rt-space-3);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-md);
    background: var(--rt-surface-1);
    color: var(--rt-fg);
    font: var(--rt-font-body);
  }
  .assertion {
    display: grid;
    grid-template-columns: auto 2fr auto 1.5fr auto;
    gap: var(--rt-space-2);
    align-items: center;
  }
  .assertion input[type="text"] {
    font-family: var(--rt-font-mono);
  }
  .muted {
    margin: 0;
    color: var(--rt-fg-muted);
  }
  .small {
    font: var(--rt-font-small);
  }
  .error {
    margin: 0;
    color: var(--rt-danger-fg);
  }
  .hint {
    padding: var(--rt-space-5) 0;
  }
</style>
