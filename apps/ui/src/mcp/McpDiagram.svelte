<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- A small picture of the two directions of MCP (wave 14): "test" — RequestTailor is the
     client and calls a server's tools, resources and prompts; "agents" — an AI assistant is
     the client and `rt mcp` serves your collection. Side by side when there is room, stacked
     in a narrow sidebar (container query). Decorative: the text next to it says the same. -->
<script lang="ts">
  import { Icon } from "../ui";

  let { direction = "test" }: { direction?: "test" | "agents" } = $props();

  const nodes = $derived(
    direction === "test"
      ? [
          { icon: "send", label: "RequestTailor", note: "the MCP client" },
          { icon: "service", label: "MCP server", note: "yours or the demo" },
        ]
      : [
          { icon: "spark", label: "AI assistant", note: "MCP client" },
          { icon: "collection", label: "rt mcp", note: "your collection" },
        ],
  );
  const offers = $derived(direction === "test" ? ["tools", "resources", "prompts"] : ["list", "send", "run"]);
</script>

<div class="diagram" aria-hidden="true" data-testid="mcp-diagram-{direction}">
  <div class="flow">
    <div class="node">
      <span class="glyph"><Icon name={nodes[0]!.icon} size="sm" /></span>
      <span class="text"><strong>{nodes[0]!.label}</strong><small>{nodes[0]!.note}</small></span>
    </div>
    <div class="link">
      <span class="line"></span>
      <span class="verb">{direction === "test" ? "calls" : "asks"}</span>
      <span class="arrow"><Icon name="chevron-right" size="sm" /></span>
    </div>
    <div class="node target">
      <span class="glyph"><Icon name={nodes[1]!.icon} size="sm" /></span>
      <span class="text"><strong>{nodes[1]!.label}</strong><small>{nodes[1]!.note}</small></span>
    </div>
  </div>
  <div class="offers">
    {#each offers as o (o)}<span class="offer">{o}</span>{/each}
  </div>
</div>

<style>
  .diagram {
    container-type: inline-size;
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-3);
    padding: var(--rt-space-4);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-lg);
    background:
      radial-gradient(120% 90% at 100% 0%, var(--rt-accent-subtle), transparent 60%),
      var(--rt-surface-sunken);
  }
  .flow {
    display: flex;
    align-items: center;
    gap: var(--rt-space-2);
  }
  .node {
    display: flex;
    align-items: center;
    gap: var(--rt-space-3);
    flex: 1 1 0;
    min-width: 0;
    padding: var(--rt-space-3);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-md);
    background: var(--rt-surface-2);
    box-shadow: var(--rt-elev-1);
  }
  .target {
    border-color: color-mix(in srgb, var(--rt-accent) 45%, var(--rt-border));
  }
  .glyph {
    display: grid;
    place-items: center;
    flex: none;
    width: var(--rt-control-sm);
    height: var(--rt-control-sm);
    border-radius: var(--rt-radius-sm);
    background: var(--rt-accent-subtle);
    color: var(--rt-accent-subtle-fg);
  }
  .text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .text strong,
  .text small {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .text strong {
    font: var(--rt-font-small);
    font-weight: var(--rt-weight-semibold);
    color: var(--rt-fg);
  }
  .text small {
    font: var(--rt-font-caption);
    text-transform: none;
    letter-spacing: 0;
    color: var(--rt-fg-muted);
  }
  .link {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    flex: none;
    color: var(--rt-fg-subtle);
  }
  .line {
    display: none;
  }
  .verb {
    font: var(--rt-font-caption);
    text-transform: none;
    letter-spacing: 0;
    color: var(--rt-fg-muted);
  }
  .offers {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: var(--rt-space-2);
  }
  .offer {
    padding: 0 var(--rt-space-3);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-pill);
    background: var(--rt-surface-2);
    font: var(--rt-font-small);
    font-family: var(--rt-font-mono);
    color: var(--rt-fg-muted);
  }

  /* A sidebar: no glyphs and notes, the nodes still side by side. */
  @container (max-width: 300px) {
    .diagram .glyph,
    .text small {
      display: none;
    }
    .node {
      justify-content: center;
      padding: var(--rt-space-2) var(--rt-space-3);
    }
    .offers {
      justify-content: center;
    }
  }

  /* Very narrow: one node above the other. */
  @container (max-width: 170px) {
    .flow {
      flex-direction: column;
      align-items: stretch;
    }
    .link {
      flex-direction: row;
      justify-content: center;
      gap: var(--rt-space-2);
    }
    .arrow {
      transform: rotate(90deg);
    }
  }
</style>
