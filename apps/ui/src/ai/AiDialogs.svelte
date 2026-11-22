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
  // Another collection: a dialog about the old one closes.
  $effect(() => {
    void app.collection?.id;
    untrack(() => {
      if (ai.view && ai.view.kind !== "settings") ai.close();
    });
  });
</script>

{#if ai.view?.kind === "settings"}
  <SettingsDialog {ai} />
{:else if ai.view?.kind === "create"}
  <CreateDialog {ai} {app} folder={ai.view.folder} />
{:else if ai.view?.kind === "explain"}
  {#key ai.view}<ExplainDialog {ai} {app} path={ai.view.path} response={ai.view.response} />{/key}
{:else if ai.view?.kind === "tests"}
  {#key ai.view}<TestsDialog {ai} {app} path={ai.view.path} response={ai.view.response} />{/key}
{/if}
