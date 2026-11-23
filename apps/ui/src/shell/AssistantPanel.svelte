<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The side panel of an extension's assistant (wave 16, strand C): beside the main area, opened
     from the header and the palette or with Mod+J. The extension draws its content; the frame
     gives the title, a close button and Escape. `focus` follows the active tab. On a narrow
     window the panel floats over the main area instead of squeezing it. -->
<script lang="ts">
  import type { AssistantFocus, ExtensionAssistant, ExtensionContext } from "../extensions";
  import { Icon, IconButton } from "../ui";

  let {
    assistant,
    ctx,
    focus,
    onclose,
  }: { assistant: ExtensionAssistant; ctx: ExtensionContext; focus: AssistantFocus; onclose: () => void } = $props();

  const Content = $derived(assistant.component);

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && !e.defaultPrevented) {
      e.preventDefault();
      e.stopPropagation();
      onclose();
    }
  }
</script>

<!-- The key handler only closes the panel when focus is inside it. -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<aside class="assistant" aria-label={assistant.title} data-testid="assistant-panel" {onkeydown}>
  <header>
    <span class="mark"><Icon name={assistant.icon ?? "spark"} size="sm" /></span>
    <h2>{assistant.title}</h2>
    <IconButton icon="close" label="Close {assistant.title}" shortcut="Esc" size="sm" data-testid="assistant-close" onclick={onclose} />
  </header>
  <div class="content">
    <Content {ctx} {focus} close={onclose} />
  </div>
</aside>

<style>
  .assistant {
    flex: none;
    display: flex;
    flex-direction: column;
    width: clamp(300px, 30vw, 440px);
    min-width: 0;
    min-height: 0;
    border-left: 1px solid var(--rt-border);
    background: var(--rt-bg-panel);
  }
  header {
    flex: none;
    display: flex;
    align-items: center;
    gap: var(--rt-space-4);
    height: var(--rt-bar-height);
    padding: 0 var(--rt-space-5);
    border-bottom: 1px solid var(--rt-border);
  }
  .mark {
    display: inline-grid;
    place-items: center;
    width: var(--rt-control-sm);
    height: var(--rt-control-sm);
    border-radius: var(--rt-radius-md);
    background: var(--rt-accent-subtle);
    color: var(--rt-accent-subtle-fg);
  }
  h2 {
    flex: 1;
    min-width: 0;
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font: var(--rt-font-title);
  }
  .content {
    flex: 1;
    min-height: 0;
    min-width: 0;
    display: flex;
    flex-direction: column;
    overflow: auto;
  }
  @media (max-width: 1100px) {
    .assistant {
      position: absolute;
      top: 0;
      right: 0;
      bottom: 0;
      z-index: 20;
      width: min(420px, calc(100vw - 48px));
      box-shadow: var(--rt-elev-3);
    }
  }
</style>
