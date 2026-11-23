<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The guided first run as a checklist in the MCP sidebar (wave 14, ./guide.ts): five steps
     from connecting to the demo server to a green "Run all". Dismissed per device; hidden by
     itself once finished. -->
<script lang="ts">
  import { Card, Icon, IconButton } from "../ui";
  import { doneCount, GUIDE_STEPS } from "./guide";
  import type { McpState } from "./mcp.svelte";

  let { mcp }: { mcp: McpState } = $props();

  const next = $derived(mcp.guideStep);
  const done = $derived(doneCount(mcp.guide));
</script>

<Card
  padding="sm"
  title={next === null ? "First run done" : "First run"}
  description={next === null ? "Your first MCP test is green. Run it in CI with rt mcp-test." : `${done} of ${GUIDE_STEPS.length} done · about a minute`}
>
  {#snippet actions()}
    <IconButton icon="close" size="sm" label="Hide the guide on this device" data-testid="mcp-guide-hide" onclick={() => mcp.hideGuide()} />
  {/snippet}
  <ol class="steps" data-testid="mcp-guide" data-next={next ?? "done"}>
    {#each GUIDE_STEPS as s, i (s.id)}
      {@const state = mcp.guide[s.id] ? "done" : s.id === next ? "next" : "todo"}
      <li class={state} data-testid="mcp-guide-step" data-step={s.id} data-state={state} aria-current={state === "next" ? "step" : undefined}>
        <span class="mark">
          {#if state === "done"}<Icon name="check" size="sm" />{:else}{i + 1}{/if}
        </span>
        <span class="title">{s.title}</span>
      </li>
    {/each}
  </ol>
  <div class="meter" role="progressbar" aria-label="Guide progress" aria-valuemin={0} aria-valuemax={GUIDE_STEPS.length} aria-valuenow={done}>
    <span style:width="{(done / GUIDE_STEPS.length) * 100}%"></span>
  </div>
</Card>

<style>
  .steps {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: flex;
    align-items: center;
    gap: var(--rt-space-3);
    min-height: var(--rt-control-sm);
    font: var(--rt-font-small);
    color: var(--rt-fg-muted);
  }
  .mark {
    display: grid;
    place-items: center;
    flex: none;
    width: 18px;
    height: 18px;
    border: 1px solid var(--rt-border-strong);
    border-radius: var(--rt-radius-pill);
    font: var(--rt-font-caption);
    letter-spacing: 0;
  }
  .done .mark {
    border-color: var(--rt-ok-border);
    background: var(--rt-ok-bg);
    color: var(--rt-ok-fg);
  }
  .done .title {
    color: var(--rt-fg-subtle);
    text-decoration: line-through;
    text-decoration-color: var(--rt-border-strong);
  }
  .next {
    color: var(--rt-fg);
    font-weight: var(--rt-weight-medium);
  }
  .next .mark {
    border-color: var(--rt-accent-solid);
    background: var(--rt-accent-solid);
    color: var(--rt-accent-solid-fg);
  }
  .meter {
    height: 4px;
    border-radius: var(--rt-radius-pill);
    background: var(--rt-surface-sunken);
    overflow: hidden;
  }
  .meter span {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--rt-accent-solid);
    transition: width var(--rt-dur-normal) var(--rt-ease-out);
  }
</style>
