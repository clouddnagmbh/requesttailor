<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Wave 18 interface: shows the open tool (`tool-host.svelte.ts`) in the main area, with a header to
     close it. Each tool is one strand's component. -->
<script lang="ts">
  import type { AppState } from "../state/app.svelte";
  import { IconButton } from "../ui";
  import type { Tools } from "./tool-host.svelte";

  let { tools, app }: { tools: Tools; app: AppState } = $props();
  const tool = $derived(tools.current);
</script>

{#if tool}
  {@const Body = tool.component}
  <section class="tool" aria-label={tool.title} data-testid="tool-{tool.id}">
    <header>
      <h2>{tool.title}</h2>
      <IconButton icon="close" label="Close {tool.title}" shortcut="Escape" tooltipPlacement="left" onclick={() => tools.close()} />
    </header>
    <Body {app} close={() => tools.close()} />
  </section>
{/if}

<style>
  .tool {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
    overflow: auto;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--rt-space-2);
    padding: var(--rt-space-2) var(--rt-space-3);
    border-bottom: 1px solid var(--rt-border);
  }
  h2 {
    margin: 0;
    font-size: var(--rt-text-md);
  }
</style>
