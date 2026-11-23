<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- "From description or snippet" (wave 8, strand E; wave 16, strand B): a sentence, or pasted
     code or API documentation text, and a folder. A cURL command is read directly, without a
     model. The proposal comes back as a "new request" suggestion (a diff to apply or discard),
     unsaved; if nothing comes of it this dialog opens again with what was typed. -->
<script lang="ts">
  import type { AppState } from "../state/app.svelte";
  import type { AiState } from "./ai.svelte";
  import AiDialog from "./AiDialog.svelte";
  import { folders } from "./apply";
  import { createFromText } from "./flows";
  import { looksLikeCurl } from "./task";

  let {
    ai,
    app,
    folder: initialFolder,
    text: initialText = "",
    mode: initialMode,
  }: { ai: AiState; app: AppState; folder: string; text?: string; mode?: "describe" | "snippet" } = $props();

  const choices = $derived(folders(app.tree));
  // svelte-ignore state_referenced_locally
  let text = $state(initialText);
  // svelte-ignore state_referenced_locally
  let mode = $state<"describe" | "snippet">(initialMode ?? "describe");
  // svelte-ignore state_referenced_locally
  let chosen = $state(initialMode !== undefined);
  // svelte-ignore state_referenced_locally
  let folder = $state(initialFolder);
  const curl = $derived(mode === "snippet" && looksLikeCurl(text));

  function typed() {
    // Pasting a cURL command switches to "snippet" unless the user picked a mode.
    if (!chosen && looksLikeCurl(text)) mode = "snippet";
  }

  async function propose() {
    const input = { mode, text, folder };
    if (text.trim() === "") return;
    ai.close();
    const done = await createFromText(app, ai, input);
    if (!done) ai.open({ kind: "create", folder, text, mode });
  }
</script>

<AiDialog title="New request from a description or snippet" testid="ai-create" onclose={() => ai.close()}>
  <div class="row" role="radiogroup" aria-label="What you give it">
    <label
      ><input
        type="radio"
        name="ai-create-mode"
        data-testid="ai-create-mode-describe"
        checked={mode === "describe"}
        onchange={() => ((mode = "describe"), (chosen = true))}
      /> Describe it</label
    >
    <label
      ><input
        type="radio"
        name="ai-create-mode"
        data-testid="ai-create-mode-snippet"
        checked={mode === "snippet"}
        onchange={() => ((mode = "snippet"), (chosen = true))}
      /> Paste a snippet</label
    >
  </div>
  <label class="field"
    >{mode === "describe" ? "What should the request do?" : "cURL command, code or API documentation text"}
    <textarea
      data-testid="ai-create-description"
      bind:value={text}
      oninput={typed}
      placeholder={mode === "describe" ? "e.g. Create an order for customer 42 with two items" : "e.g. curl -X POST https://api.example.com/orders -d '{\"sku\": \"a-1\"}'"}
      onkeydown={(e) => {
        if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) void propose();
      }}
    ></textarea>
  </label>
  <label class="field"
    >Folder
    <select data-testid="ai-create-folder" bind:value={folder}>
      {#each choices as f (f.path)}<option value={f.path}>{f.label}</option>{/each}
    </select>
  </label>
  {#if curl}
    <p class="muted" data-testid="ai-create-local">A cURL command is read directly. No model is called and nothing leaves your computer.</p>
  {:else}
    <p class="muted">
      The model sees this folder's requests with secret values masked, and the names of your variables. You see exactly what is sent before it is.
    </p>
  {/if}
  <div class="row">
    <button class="primary" data-testid="ai-create-propose" disabled={text.trim() === ""} onclick={propose}>{curl ? "Read command" : "Propose"}</button>
    <button data-testid="ai-create-cancel" onclick={() => ai.close()}>Cancel</button>
  </div>
</AiDialog>
