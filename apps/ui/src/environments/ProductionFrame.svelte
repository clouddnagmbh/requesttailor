<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Production protection of the shell (wave 17, strand D): a red frame around the window while
     the active environment is a production system, the question before a writing request
     (ConfirmSend) and before a run. It never blocks reading requests and never changes what
     is sent. Mount it once in the shell. -->
<script lang="ts">
  import { getApp } from "../state/app.svelte";
  import ConfirmSend from "./ConfirmSend.svelte";
  import { getProduction } from "./production.svelte";

  const app = getApp();
  const production = getProduction(app);

  // The policy follows the collection, a reload and edits of opencollection.yml.
  $effect(() => {
    void app.collection?.id;
    void app.loadCount;
    void app.dirty.length;
    void production.refresh();
  });

  $effect(() => {
    app.sendConfirm = (path) => production.gate(path);
    return () => (app.sendConfirm = null);
  });
</script>

{#if production.active}
  <div class="frame" data-testid="production-frame" role="status" aria-label="Production environment {app.environment}">
    <span class="tag">PRODUCTION · {app.environment}</span>
  </div>
{/if}
<ConfirmSend />

<style>
  .frame {
    position: fixed;
    inset: 0;
    z-index: 40;
    box-sizing: border-box;
    border: 3px solid var(--rt-danger-solid);
    pointer-events: none;
  }
  .tag {
    position: absolute;
    top: 0;
    left: 50%;
    transform: translateX(-50%);
    padding: 0 var(--rt-space-4);
    border-radius: 0 0 var(--rt-radius-md) var(--rt-radius-md);
    background: var(--rt-danger-solid);
    color: #fff;
    font-size: var(--rt-text-xs, 11px);
    font-weight: 700;
    letter-spacing: 0.06em;
    white-space: nowrap;
  }
</style>
