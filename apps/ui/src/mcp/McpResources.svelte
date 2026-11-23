<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The connected server's resources (wave 14): the list (all pages) and the contents of the
     one opened, text as is (JSON pretty), binary data by size. Masked by the core. -->
<script lang="ts">
  import { Badge, Button, EmptyState, Spinner, Tooltip } from "../ui";
  import type { McpState } from "./mcp.svelte";
  import { pretty } from "./model";

  let { mcp }: { mcp: McpState } = $props();

  function shown(text: string, mime: string | null): string {
    if (mime?.includes("json")) {
      try {
        return pretty(JSON.parse(text));
      } catch {
        return text;
      }
    }
    return text;
  }
</script>

{#if mcp.resources.length === 0}
  <EmptyState size="sm" icon="file-text" title="No resources" text="This server offers no resources: files or data a client can read." />
{:else}
  <div class="split">
    <ul class="list" role="listbox" aria-label="Resources" data-testid="mcp-resources" data-count={mcp.resources.length}>
      {#each mcp.resources as r (r.uri)}
        <li
          role="option"
          tabindex="0"
          aria-selected={mcp.resourceUri === r.uri}
          data-testid="mcp-resource"
          data-uri={r.uri}
          onclick={() => mcp.openResource(r.uri)}
          onkeydown={(e) => (e.key === "Enter" || e.key === " ") && (e.preventDefault(), mcp.openResource(r.uri))}
        >
          <span class="name">{r.title ?? r.name}</span>
          {#if r.mime_type}<Badge value={r.mime_type} />{/if}
          <span class="uri">{r.uri}</span>
          {#if r.description}<span class="desc">{r.description}</span>{/if}
        </li>
      {/each}
    </ul>
    <div class="view">
      {#if mcp.browsing && mcp.resourceUri}
        <Spinner label="Reading" />
      {:else if mcp.resourceContents && mcp.resourceUri}
        <div class="head">
          <span class="uri">{mcp.resourceUri}</span>
          <Tooltip text="Read the resource again (resources can change, like demo://todos)">
            <Button size="sm" variant="ghost" icon="sync" onclick={() => mcp.resourceUri && mcp.openResource(mcp.resourceUri)}>Reload</Button>
          </Tooltip>
        </div>
        {#each mcp.resourceContents as c, i (i)}
          {#if c.text !== null}
            <pre data-testid="mcp-resource-text">{shown(c.text, c.mime_type)}</pre>
          {:else if c.blob !== null}
            <p class="muted">Binary data ({c.mime_type ?? "unknown type"}, {c.blob.length} base64 characters)</p>
          {/if}
        {/each}
      {:else}
        <p class="muted hint">Choose a resource to read it.</p>
      {/if}
    </div>
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
  li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-1) var(--rt-space-3);
    padding: var(--rt-space-3) var(--rt-space-4);
    border-radius: var(--rt-radius-md);
    cursor: pointer;
  }
  li:hover {
    background: var(--rt-bg-hover);
  }
  li[aria-selected="true"] {
    background: var(--rt-accent-subtle);
    color: var(--rt-accent-subtle-fg);
  }
  .name {
    font-weight: var(--rt-weight-semibold);
  }
  .uri {
    width: 100%;
    font: var(--rt-font-small);
    font-family: var(--rt-font-mono);
    color: var(--rt-fg-muted);
    overflow-wrap: anywhere;
  }
  .head .uri {
    width: auto;
    flex: 1;
  }
  .desc {
    width: 100%;
    font: var(--rt-font-small);
    color: var(--rt-fg-muted);
  }
  .view {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-3);
  }
  .head {
    display: flex;
    align-items: center;
    gap: var(--rt-space-3);
  }
  pre {
    margin: 0;
    padding: var(--rt-space-3) var(--rt-space-4);
    background: var(--rt-surface-sunken);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-md);
    font: var(--rt-font-small);
    font-family: var(--rt-font-mono);
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 60vh;
    overflow: auto;
  }
  .muted {
    margin: 0;
    color: var(--rt-fg-muted);
  }
  .hint {
    padding: var(--rt-space-5) 0;
  }
</style>
