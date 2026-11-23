<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The assistant panel of the wave 16 demo extension (dev server only): shows what the panel
     is told to look at, and calls `ctx.ai.complete` with the preview so its cancel path can be
     tried. No provider is involved in the tests (they replace the backend's AI calls). -->
<script lang="ts">
  import type { AssistantPanelProps } from "@requesttailor/ui";
  import { Button, EmptyState } from "@requesttailor/ui";

  let { ctx, focus, close }: AssistantPanelProps = $props();

  let answer = $state<string | null>(null);
  let problem = $state<string | null>(null);
  let controller = $state<AbortController | null>(null);

  async function ask() {
    answer = null;
    problem = null;
    controller = new AbortController();
    try {
      const reply = await ctx.ai.complete(
        { purpose: "Demo question", system: "You are a demo.", messages: [{ role: "user", content: `About ${focus.request ?? "nothing"}` }] },
        { preview: true, signal: controller.signal },
      );
      answer = reply.text;
    } catch (e) {
      problem = e instanceof Error ? `${(e as { kind?: string }).kind ?? "error"}: ${e.message}` : String(e);
    } finally {
      controller = null;
    }
  }
</script>

<div class="panel" data-testid="demo16-panel">
  {#if focus.request}
    <p class="focus">Looking at <code data-testid="demo16-focus">{focus.request}</code></p>
    <p class="muted">{focus.response ? `Last response: ${focus.response.report.response?.status ?? "none"}` : "Not sent yet."}</p>
    <div class="row">
      <Button variant="primary" data-testid="demo16-ask" loading={controller !== null} onclick={ask}>Ask with preview</Button>
      {#if controller}<Button data-testid="demo16-stop" onclick={() => controller?.abort()}>Stop</Button>{/if}
      <Button variant="ghost" onclick={close}>Close</Button>
    </div>
    {#if answer}<p data-testid="demo16-answer">{answer}</p>{/if}
    {#if problem}<p class="muted" data-testid="demo16-problem">{problem}</p>{/if}
  {:else}
    <EmptyState icon="spark" title="No request open" text="Open a request and this panel follows it." size="sm" />
  {/if}
</div>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-4);
    padding: var(--rt-space-5);
  }
  p {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .muted {
    color: var(--rt-fg-muted);
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--rt-space-3);
  }
</style>
