<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The connected server's prompts (wave 14): the list (all pages), a field per argument
     ({{variables}} allowed) and the messages the prompt would put in front of a model. -->
<script lang="ts">
  import { Badge, Button, EmptyState, TextField, Tooltip } from "../ui";
  import type { McpState } from "./mcp.svelte";

  let { mcp }: { mcp: McpState } = $props();

  const prompt = $derived(mcp.prompts.find((p) => p.name === mcp.promptName) ?? null);
  const missing = $derived(prompt ? prompt.arguments.filter((a) => a.required && (mcp.promptArgs[a.name] ?? "").trim() === "").map((a) => a.name) : []);
</script>

{#if mcp.prompts.length === 0}
  <EmptyState size="sm" icon="spark" title="No prompts" text="This server offers no prompts: message templates a client can fill in for a model." />
{:else}
  <div class="split">
    <ul class="list" role="listbox" aria-label="Prompts" data-testid="mcp-prompts" data-count={mcp.prompts.length}>
      {#each mcp.prompts as p (p.name)}
        <li
          role="option"
          tabindex="0"
          aria-selected={mcp.promptName === p.name}
          data-testid="mcp-prompt"
          data-name={p.name}
          onclick={() => mcp.selectPrompt(p.name)}
          onkeydown={(e) => (e.key === "Enter" || e.key === " ") && (e.preventDefault(), mcp.selectPrompt(p.name))}
        >
          <span class="name">{p.title ?? p.name}</span>
          {#if p.description}<span class="desc">{p.description}</span>{/if}
        </li>
      {/each}
    </ul>
    {#if prompt}
      <form
        class="view"
        onsubmit={(e) => {
          e.preventDefault();
          void mcp.getPrompt();
        }}
      >
        {#each prompt.arguments as a (a.name)}
          <TextField
            label="{a.title ?? a.name}{a.required ? ' *' : ''}"
            hint={a.description ?? undefined}
            data-testid="mcp-prompt-arg"
            data-name={a.name}
            bind:value={mcp.promptArgs[a.name]}
          />
        {/each}
        <div class="row">
          <Tooltip text={missing.length > 0 ? `Fill in ${missing.join(", ")} first` : "Ask the server for the filled-in messages (prompts/get)"}>
            <Button type="submit" variant="primary" data-testid="mcp-prompt-get" disabled={missing.length > 0} loading={mcp.browsing}>Get prompt</Button>
          </Tooltip>
        </div>
        {#if mcp.promptResult}
          <ol class="messages" data-testid="mcp-prompt-messages">
            {#each mcp.promptResult.messages as m, i (i)}
              <li>
                <Badge value={m.role} tone={m.role === "assistant" ? "accent" : "neutral"} />
                {#if m.content.type === "text"}
                  <pre>{m.content.text}</pre>
                {:else if m.content.type === "resource"}
                  <pre><span class="uri">{m.content.uri}</span>{"\n"}{m.content.text ?? ""}</pre>
                {:else}
                  <pre>{JSON.stringify(m.content, null, 2)}</pre>
                {/if}
              </li>
            {/each}
          </ol>
        {/if}
      </form>
    {/if}
  </div>
{/if}

<style>
  .split {
    display: flex;
    gap: var(--rt-space-5);
    align-items: flex-start;
  }
  .list {
    flex: none;
    width: 260px;
    max-height: 60vh;
    margin: 0;
    padding: var(--rt-space-2);
    overflow: auto;
    list-style: none;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-lg);
    background: var(--rt-surface-2);
  }
  .list li {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-1);
    padding: var(--rt-space-3) var(--rt-space-4);
    border-radius: var(--rt-radius-md);
    cursor: pointer;
  }
  .list li:hover {
    background: var(--rt-bg-hover);
  }
  .list li[aria-selected="true"] {
    background: var(--rt-accent-subtle);
    color: var(--rt-accent-subtle-fg);
  }
  .name {
    font-weight: var(--rt-weight-semibold);
  }
  .desc {
    font: var(--rt-font-small);
    color: var(--rt-fg-muted);
  }
  .view {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-4);
  }
  .row {
    display: flex;
    gap: var(--rt-space-3);
  }
  .messages {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-4);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .messages li {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--rt-space-2);
  }
  pre {
    align-self: stretch;
    margin: 0;
    padding: var(--rt-space-3) var(--rt-space-4);
    background: var(--rt-surface-sunken);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-md);
    font: var(--rt-font-small);
    font-family: var(--rt-font-mono);
    white-space: pre-wrap;
    word-break: break-word;
  }
  .uri {
    color: var(--rt-fg-muted);
  }
</style>
