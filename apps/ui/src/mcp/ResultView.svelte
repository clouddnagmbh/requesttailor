<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The result of an MCP tool call (wave 8, strand D): tool error flag, time, structured
     output as JSON, then each content item (text, images, audio, resources, others).
     Values are masked by the core. -->
<script lang="ts">
  import type { McpCallResult } from "../commands/types";
  import { formatDuration } from "../response/format";
  import { pretty } from "./model";

  let { result }: { result: McpCallResult } = $props();

  /** Images only as `data:` URLs of image types; nothing is fetched. */
  const safeImage = (mime: string) => /^image\/(png|jpeg|gif|webp)$/.test(mime);
</script>

<div class="result" data-testid="mcp-result" data-error={result.is_error}>
  <div class="head">
    {#if result.is_error}
      <span class="bad" data-testid="mcp-result-state">Tool error</span>
    {:else}
      <span class="ok" data-testid="mcp-result-state">OK</span>
    {/if}
    <span class="muted" data-testid="mcp-result-time">{formatDuration(result.time_ms)}</span>
  </div>
  {#if result.structured !== null}
    <h4>Structured output</h4>
    <pre data-testid="mcp-result-structured">{pretty(result.structured)}</pre>
  {/if}
  {#each result.content as c, i (i)}
    {#if c.type === "text"}
      <pre data-testid="mcp-result-text">{c.text}</pre>
    {:else if c.type === "image"}
      {#if safeImage(c.mime_type)}
        <img data-testid="mcp-result-image" src="data:{c.mime_type};base64,{c.data}" alt="Image from the tool ({c.mime_type})" />
      {:else}
        <p class="muted">Image ({c.mime_type}, {c.data.length} base64 characters)</p>
      {/if}
    {:else if c.type === "audio"}
      <p class="muted" data-testid="mcp-result-audio">Audio ({c.mime_type}, {c.data.length} base64 characters)</p>
    {:else if c.type === "resource"}
      <div data-testid="mcp-result-resource">
        <span class="mono">{c.uri}</span>
        {#if c.text !== null}<pre>{c.text}</pre>{/if}
      </div>
    {:else}
      <pre data-testid="mcp-result-other">{pretty(c.value)}</pre>
    {/if}
  {/each}
  {#if result.content.length === 0 && result.structured === null}
    <p class="muted">No content.</p>
  {/if}
</div>

<style>
  .result {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-height: 0;
  }
  .head {
    display: flex;
    gap: 12px;
    align-items: baseline;
  }
  h4 {
    margin: 4px 0 0;
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
  }
  pre {
    margin: 0;
    padding: 6px 8px;
    background: var(--rt-bg-panel);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    font-family: var(--rt-font-mono);
    font-size: var(--rt-size-small);
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 320px;
    overflow: auto;
  }
  img {
    max-width: 100%;
    max-height: 320px;
    align-self: flex-start;
    image-rendering: pixelated;
    border: 1px solid var(--rt-border);
    min-width: 16px;
    min-height: 16px;
  }
  .mono {
    font-family: var(--rt-font-mono);
  }
  .ok {
    color: var(--rt-ok);
    font-weight: 600;
  }
  .bad {
    color: var(--rt-danger);
    font-weight: 600;
  }
  .muted {
    color: var(--rt-fg-muted);
    margin: 0;
  }
</style>
