<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The result of an MCP tool call (wave 8, strand D): tool error flag, time, structured
     output as JSON, then each content item (text, images, audio, resources, others).
     Wave 14: the notifications that came before the result (progress, logs), in order.
     Values are masked by the core. -->
<script lang="ts">
  import type { McpCallResult } from "../commands/types";
  import { formatDuration } from "../response/format";
  import { Badge, Progress, Section } from "../ui";
  import { notificationLine, pretty } from "./model";

  let { result }: { result: McpCallResult } = $props();

  /** Images only as `data:` URLs of image types; nothing is fetched. */
  const safeImage = (mime: string) => /^image\/(png|jpeg|gif|webp)$/.test(mime);
  const notes = $derived((result.notifications ?? []).map(notificationLine));
</script>

<div class="result" data-testid="mcp-result" data-error={result.is_error}>
  <div class="head">
    <span data-testid="mcp-result-state" class="state">{result.is_error ? "Tool error" : "OK"}</span>
    <Badge value={result.is_error ? "isError: true" : "result"} tone={result.is_error ? "danger" : "ok"} />
    <span class="muted" data-testid="mcp-result-time">{formatDuration(result.time_ms)}</span>
  </div>
  {#if notes.length > 0}
    <Section title="Notifications" count={notes.length} description="Sent by the server while the tool ran, before its result.">
      <ol class="notes" data-testid="mcp-result-notifications">
        {#each notes as n, i (i)}
          <li data-testid="mcp-result-notification" data-kind={n.kind}>
            <span class="kind">{n.label}</span>
            {#if n.progress !== null}
              <span class="bar"><Progress size="sm" value={n.progress} max={1} label="Progress" /></span>
            {/if}
            <span class="text">{n.text}</span>
          </li>
        {/each}
      </ol>
    </Section>
  {/if}
  {#if result.structured !== null}
    <Section title="Structured output">
      <pre data-testid="mcp-result-structured">{pretty(result.structured)}</pre>
    </Section>
  {/if}
  <Section title="Content" count={result.content.length}>
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
  </Section>
</div>

<style>
  .result {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-5);
    min-height: 0;
  }
  .head {
    display: flex;
    gap: var(--rt-space-3);
    align-items: center;
  }
  .state {
    font: var(--rt-font-title);
    color: var(--rt-fg);
  }
  .result[data-error="true"] .state {
    color: var(--rt-danger-fg);
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
    max-height: 320px;
    overflow: auto;
  }
  img {
    align-self: flex-start;
    max-width: 100%;
    max-height: 320px;
    min-width: 16px;
    min-height: 16px;
    padding: var(--rt-space-3);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-md);
    background: var(--rt-surface-sunken);
  }
  .notes {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-1);
    margin: 0;
    padding: 0;
    list-style: none;
    font: var(--rt-font-small);
  }
  .notes li {
    display: flex;
    align-items: center;
    gap: var(--rt-space-3);
    min-height: var(--rt-control-sm);
  }
  .kind {
    flex: none;
    min-width: 64px;
    font-family: var(--rt-font-mono);
    color: var(--rt-fg-muted);
  }
  .bar {
    flex: none;
    width: 64px;
  }
  .text {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .mono {
    font-family: var(--rt-font-mono);
  }
  .muted {
    color: var(--rt-fg-muted);
    margin: 0;
  }
</style>
