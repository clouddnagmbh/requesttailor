<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The plugin catalogue as a view of the main area (`ToolId` "catalogue", palette "Plugin
     catalogue"): the same panel as the "Catalogue" tab of the plugins dialog. -->
<script lang="ts">
  import type { ToolProps } from "../../shell/tool-host.svelte";
  import { Button } from "../../ui";
  import CataloguePanel from "./CataloguePanel.svelte";

  let { close }: ToolProps = $props();
</script>

<div class="view" data-testid="catalogue-view">
  <header>
    <h2>Plugin catalogue</h2>
    <Button variant="ghost" onclick={close} data-testid="catalogue-view-close">Close</Button>
  </header>
  <CataloguePanel />
</div>

<style>
  .view {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-3);
    padding: var(--rt-space-4);
    max-width: 760px;
    margin: 0 auto;
    overflow: auto;
    height: 100%;
    box-sizing: border-box;
  }
  header {
    display: flex;
    align-items: center;
  }
  h2 {
    flex: 1;
    margin: 0;
    font-size: var(--rt-text-lg, 1.1em);
  }
</style>
