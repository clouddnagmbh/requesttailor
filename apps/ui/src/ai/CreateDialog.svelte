<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- "Create with AI" (wave 8, strand E): a description and a folder, then the proposed
     request to confirm. Confirming creates it unsaved in that folder and opens it; nothing is
     saved until the user saves. -->
<script lang="ts">
  import type { AiAnswer, RequestSpec } from "../commands/types";
  import type { AppState } from "../state/app.svelte";
  import type { AiState } from "./ai.svelte";
  import AiDialog from "./AiDialog.svelte";
  import { askError, folders, withProposal } from "./apply";
  import Markdown from "./Markdown.svelte";

  let { ai, app, folder: initialFolder }: { ai: AiState; app: AppState; folder: string } = $props();

  const choices = $derived(folders(app.tree));
  let description = $state("");
  // svelte-ignore state_referenced_locally
  let folder = $state(initialFolder);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let answer = $state.raw<(AiAnswer & { request: RequestSpec }) | null>(null);

  async function propose() {
    if (description.trim() === "") return;
    busy = true;
    error = null;
    try {
      const a = await ai.backend.ai.ask(
        { task: "create-request", description: description.trim(), folder },
        { environment: app.environment, vars: [] },
      );
      if (!a.request) throw new Error("The answer had no request.");
      answer = { ...a, request: a.request };
    } catch (e) {
      error = askError(e);
    } finally {
      busy = false;
    }
  }

  async function confirm() {
    if (!answer) return;
    busy = true;
    error = null;
    try {
      const path = await app.backend.createRequest(folder, answer.request.name || "New request");
      const base = await app.backend.getRequest(path);
      await app.edit(path, withProposal(base, answer.request));
      await app.select(path);
      ai.close();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  const bodyText = (r: RequestSpec) =>
    r.body.type === "raw"
      ? r.body.data
      : r.body.type === "form-url-encoded"
        ? r.body.fields.map((f) => `${f.name}=${f.value}`).join("\n")
        : "";
</script>

<AiDialog title="Create a request with AI" testid="ai-create" {busy} onclose={() => ai.close()}>
  {#if !answer}
    <label class="field"
      >What should the request do?
      <textarea
        data-testid="ai-create-description"
        bind:value={description}
        placeholder="e.g. Create an order for customer 42 with two items"
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
    <p class="muted">The model sees this folder's requests with secret values masked, and the names of your variables.</p>
    {#if error}<p class="error" role="alert" data-testid="ai-create-error">{error}</p>{/if}
    <div class="row">
      <button class="primary" data-testid="ai-create-propose" disabled={busy || description.trim() === ""} onclick={propose}
        >{busy ? "Asking…" : "Propose"}</button
      >
      <button data-testid="ai-create-cancel" disabled={busy} onclick={() => ai.close()}>Cancel</button>
    </div>
  {:else}
    {@const r = answer.request}
    <p>Create this request in <strong>{choices.find((f) => f.path === folder)?.label ?? folder}</strong>? It stays unsaved until you save.</p>
    {#if answer.text}<Markdown text={answer.text} testid="ai-create-text" />{/if}
    <dl class="proposal" data-testid="ai-proposal">
      <dt>Name</dt>
      <dd data-testid="ai-proposal-name">{r.name}</dd>
      <dt>Request</dt>
      <dd><code data-testid="ai-proposal-line">{r.method} {r.url}</code></dd>
      {#if r.headers.length > 0}
        <dt>Headers</dt>
        <dd><pre>{r.headers.map((h) => `${h.name}: ${h.value}`).join("\n")}</pre></dd>
      {/if}
      {#if bodyText(r)}
        <dt>Body</dt>
        <dd><pre>{bodyText(r)}</pre></dd>
      {/if}
      <dt>Auth</dt>
      <dd>inherited from the folder or collection</dd>
    </dl>
    {#if error}<p class="error" role="alert" data-testid="ai-create-error">{error}</p>{/if}
    <div class="row">
      <button class="primary" data-testid="ai-create-confirm" disabled={busy} onclick={confirm}>Create request</button>
      <button data-testid="ai-create-back" disabled={busy} onclick={() => (answer = null)}>Back</button>
      <button data-testid="ai-create-cancel" disabled={busy} onclick={() => ai.close()}>Cancel</button>
    </div>
  {/if}
</AiDialog>

<style>
  .proposal {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr);
    gap: 4px 12px;
    margin: 0;
  }
  dt {
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
  dd {
    margin: 0;
    min-width: 0;
  }
  code {
    font-family: var(--rt-font-mono);
    word-break: break-all;
  }
</style>
