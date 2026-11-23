<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Name proposals (wave 16, strand B). A request's name is chosen and shown as a diff next;
     folders and variables have no rename in the app, so their names are copied. -->
<script lang="ts">
  import type { ExtensionDialogProps } from "../extensions";
  import { Button, DialogFooter } from "../ui";

  let { close, props }: ExtensionDialogProps = $props();
  const names = $derived(props.names as string[]);
  const target = $derived(props.target as string);
  let copied = $state<string | null>(null);

  async function copy(name: string) {
    try {
      await navigator.clipboard.writeText(name);
      copied = name;
    } catch {
      copied = null;
    }
  }
</script>

<div class="names">
  <p class="muted">
    {#if target === "request"}Pick one to see the change before it is applied.{:else}Copy one, then use it where you rename or define it.{/if}
  </p>
  <ul data-testid="ai-names">
    {#each names as name (name)}
      <li>
        <span class="name" data-testid="ai-name">{name}</span>
        {#if target === "request"}
          <Button size="sm" data-testid="ai-name-use" onclick={() => close({ name })}>Use</Button>
        {:else}
          <Button size="sm" data-testid="ai-name-copy" onclick={() => copy(name)}>{copied === name ? "Copied" : "Copy"}</Button>
        {/if}
      </li>
    {/each}
  </ul>
  <DialogFooter>
    <Button data-testid="ai-names-close" onclick={() => close()}>Close</Button>
  </DialogFooter>
</div>

<style>
  ul {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-3);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--rt-space-4);
    padding: var(--rt-space-3) var(--rt-space-4);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-md);
  }
  .name {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .muted {
    margin: 0;
    color: var(--rt-fg-muted);
    font: var(--rt-font-caption);
  }
</style>
