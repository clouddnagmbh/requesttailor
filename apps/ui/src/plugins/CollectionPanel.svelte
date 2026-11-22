<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The collection tabs of enabled plugins (`place: "collection"`, wave 6): a panel of the
     open collection in the main area, one tab per plugin tab, rendered by `PluginTab` with
     `request_path` "". The shell opens it from the header button "Collection" or the command
     palette; without collection tabs the shell shows neither. Owner: wave 6, strand F. -->
<script lang="ts">
  import { getApp } from "../state/app.svelte";
  import type { PluginTabRef } from "./permissions";
  import PluginTab from "./PluginTab.svelte";

  let {
    tabs,
    active,
    onselect,
    onclose,
  }: {
    tabs: PluginTabRef[];
    /** `PluginTabRef.key` of the shown tab. */
    active: string;
    onselect: (key: string) => void;
    onclose: () => void;
  } = $props();

  const app = getApp();
  const current = $derived(tabs.find((t) => t.key === active) ?? tabs[0]);
</script>

<section class="panel" data-testid="collection-panel" aria-label="Collection">
  <div class="bar" role="tablist" aria-label="Collection tabs">
    {#each tabs as t (t.key)}
      <button
        role="tab"
        class="tab"
        data-testid="collection-tab"
        data-tab={t.key}
        title="Plugin {t.pluginName}"
        aria-selected={t.key === current?.key}
        onclick={() => onselect(t.key)}>{t.tab.title}</button
      >
    {/each}
    <span class="spacer"></span>
    <button class="close" data-testid="collection-panel-close" aria-label="Close collection panel" onclick={onclose}>×</button>
  </div>
  {#if current}
    {#key current.key}
      <div class="content" role="tabpanel">
        <PluginTab backend={app.backend} plugin={current.plugin} tab={current.tab.id} target={{ request_path: "", response_id: null }} />
      </div>
    {/key}
  {/if}
</section>

<style>
  .panel {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 4px 8px 0;
    border-bottom: 1px solid var(--rt-border);
  }
  .tab {
    padding: 3px 10px;
    border: none;
    border-bottom: 2px solid transparent;
    background: none;
    color: var(--rt-fg-muted);
    font: inherit;
    cursor: pointer;
  }
  .tab[aria-selected="true"] {
    border-bottom-color: var(--rt-accent);
    color: var(--rt-fg);
  }
  .spacer {
    flex: 1;
  }
  .close {
    border: none;
    background: none;
    color: var(--rt-fg-muted);
    font: inherit;
    font-size: 1.2em;
    cursor: pointer;
  }
  .content {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
</style>
