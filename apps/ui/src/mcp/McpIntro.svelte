<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- What the MCP view is for, in plain language (wave 14): the first-visit state of the MCP
     sidebar, with a small diagram, the three primary actions ("Try the demo server",
     "Connect to a server…", "Learn more") and the other direction (`rt mcp`, "Connect to
     RequestTailor itself"). Fits 200 px and grows to 400 px. -->
<script lang="ts">
  import { Button, Card, Icon, Tooltip } from "../ui";
  import McpDiagram from "./McpDiagram.svelte";
  import type { McpState } from "./mcp.svelte";
  import { LEARN_MORE_URL } from "./model";

  let { mcp }: { mcp: McpState } = $props();

  const STEPS = [
    ["Connect", "to a server, by command (stdio) or URL"],
    ["Browse", "its tools, resources and prompts"],
    ["Call", "a tool with a form and see the result"],
    ["Save", "the call as a test, a *.mcp.yaml file next to your collection"],
    ["Run all", "tests here, and in CI with rt mcp-test"],
  ] as const;
</script>

<div class="intro" data-testid="mcp-intro">
  <McpDiagram direction="test" />
  <div class="copy">
    <h3>Test MCP servers like APIs</h3>
    <p>MCP (Model Context Protocol) is how AI assistants call tools on servers. Here you are the assistant.</p>
  </div>
  <ol class="how">
    {#each STEPS as [verb, rest], i (verb)}
      <li><span class="n">{i + 1}</span><span><strong>{verb}</strong> {rest}</span></li>
    {/each}
  </ol>
  <div class="actions">
    <Tooltip text="Connect to a demo server built into RequestTailor: fake data, offline, nothing to install">
      <Button variant="primary" icon="play" block data-testid="mcp-try-demo" loading={mcp.connecting && mcp.demo} onclick={() => mcp.tryDemo()}>Try the demo server</Button>
    </Tooltip>
    <Tooltip text="Enter the command or URL of any MCP server">
      <Button icon="add" block data-testid="mcp-new" onclick={() => mcp.newConnection()}>Connect to a server…</Button>
    </Tooltip>
    <Tooltip text="Opens the guide to MCP in RequestTailor in your browser">
      <a class="learn" href={LEARN_MORE_URL} target="_blank" rel="noopener noreferrer" data-testid="mcp-learn-more">
        Learn more <Icon name="external-link" size="sm" />
      </a>
    </Tooltip>
  </div>

  <Card padding="sm" title="The other direction" description="rt mcp makes your collections available to AI agents, with read/write rules and approvals.">
    <Tooltip text="Start rt mcp on this collection and look at it the way an AI agent does">
      <Button size="sm" icon="mcp" block data-testid="mcp-try-self" onclick={() => mcp.trySelf()}>Connect to RequestTailor itself</Button>
    </Tooltip>
  </Card>
</div>

<style>
  .intro {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-5);
  }
  .copy {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-2);
  }
  h3 {
    margin: 0;
    font: var(--rt-font-title);
    letter-spacing: var(--rt-tracking-tight);
    color: var(--rt-fg);
  }
  p {
    margin: 0;
    color: var(--rt-fg-muted);
  }
  .how {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-2);
    margin: 0;
    padding: 0;
    list-style: none;
    font: var(--rt-font-small);
    color: var(--rt-fg-muted);
  }
  .how li {
    display: flex;
    align-items: baseline;
    gap: var(--rt-space-3);
  }
  .how strong {
    color: var(--rt-fg);
    font-weight: var(--rt-weight-semibold);
  }
  .n {
    display: inline-grid;
    place-items: center;
    flex: none;
    width: 16px;
    height: 16px;
    border-radius: var(--rt-radius-pill);
    background: var(--rt-surface-sunken);
    font: var(--rt-font-caption);
    letter-spacing: 0;
    color: var(--rt-fg-muted);
    transform: translateY(2px);
  }
  .actions {
    display: grid;
    gap: var(--rt-space-3);
  }
  .actions > :global(*) {
    width: 100%;
  }
  .learn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--rt-space-2);
    width: 100%;
    font: var(--rt-font-small);
  }
</style>
