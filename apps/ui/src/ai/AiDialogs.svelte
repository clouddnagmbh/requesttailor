<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The AI assistant's dialogs (wave 8, strand E), mounted once by the shell. Which one is open
     is `AiState.view`; the buttons and palette entries only set it. -->
<script lang="ts">
  import { untrack } from "svelte";

  import { getApp } from "../state/app.svelte";
  import { aiFor } from "./ai.svelte";
  import CreateDialog from "./CreateDialog.svelte";
  import ExplainDialog from "./ExplainDialog.svelte";
  import SettingsDialog from "./SettingsDialog.svelte";
  import TestsDialog from "./TestsDialog.svelte";

  const app = getApp();
  const ai = aiFor(app.backend);
  void ai.ensure();
  // Another collection: a dialog about the old one closes, and the policy is read again.
  $effect(() => {
    void app.collection?.id;
    untrack(() => {
      void ai.refreshPolicy();
      if (ai.view && ai.view.kind !== "settings") ai.close();
    });
  });
</script>

{#if ai.view?.kind === "settings"}
  <SettingsDialog {ai} />
{:else if ai.view?.kind === "create"}
  {#key ai.view}<CreateDialog {ai} {app} folder={ai.view.folder} text={ai.view.text} mode={ai.view.mode} />{/key}
{:else if ai.view?.kind === "explain"}
  {#key ai.view}<ExplainDialog {ai} {app} path={ai.view.path} response={ai.view.response} />{/key}
{:else if ai.view?.kind === "tests"}
  {#key ai.view}<TestsDialog {ai} {app} path={ai.view.path} response={ai.view.response} />{/key}
{/if}

{#if ai.working}
  <p class="working" role="status" aria-live="polite" data-testid="ai-working">{ai.working}</p>
{/if}

<style>
  .working {
    position: fixed;
    right: var(--rt-space-5);
    bottom: var(--rt-space-5);
    z-index: 50;
    margin: 0;
    padding: var(--rt-space-3) var(--rt-space-5);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-md);
    background: var(--rt-surface-3);
    color: var(--rt-fg);
    box-shadow: var(--rt-elev-2);
    font: var(--rt-font-caption);
  }
</style>
