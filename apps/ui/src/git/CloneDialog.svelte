<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- "Clone a repository" as a dialog, reachable without an open collection (start screen,
     command palette). `onopen` shows the collection the backend opened (as after Open folder).
     Private repositories ask for credentials once and retry (`git.setCredentials`). -->
<script lang="ts">
  import { untrack } from "svelte";

  import { BackendError, type CollectionInfo } from "../commands/backend";
  import { getApp } from "../state/app.svelte";
  import CloneFields from "./CloneFields.svelte";

  let {
    open,
    onclose,
    onopen,
  }: { open: boolean; onclose: () => void; onopen: (info: CollectionInfo) => Promise<void> } = $props();
  const app = getApp();
  const git = app.backend.git;

  let url = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);
  let credentials = $state<string | null>(null);
  let user = $state("");
  let password = $state("");

  function reset() {
    url = "";
    busy = false;
    error = null;
    credentials = null;
    user = "";
    password = "";
  }

  function opened(node: HTMLDialogElement) {
    node.showModal();
    untrack(reset);
    return () => node.close();
  }

  async function clone() {
    const address = url.trim();
    if (!address || busy) return;
    busy = true;
    error = null;
    try {
      if (credentials) {
        await git.setCredentials(credentials, user.trim(), password);
        credentials = null;
        password = "";
      }
      const info = await git.clone(address);
      if (info) {
        onclose();
        await onopen(info);
      }
    } catch (e) {
      if (e instanceof BackendError && e.kind === "git-auth") {
        credentials = e.message;
        error = `${e.message} needs a user name and a password or access token.`;
      } else {
        error = e instanceof Error ? e.message : String(e);
      }
    } finally {
      busy = false;
    }
  }
</script>

{#if open}
  <dialog
    data-testid="clone-dialog"
    aria-labelledby="clone-title"
    {@attach opened}
    oncancel={(e) => {
      e.preventDefault();
      onclose();
    }}
  >
    <form
      onsubmit={(e) => {
        e.preventDefault();
        void clone();
      }}
    >
      <h2 id="clone-title">Clone a repository</h2>
      <CloneFields bind:url disabled={busy} label="Repository address" />
      {#if credentials}
        <label>
          User name
          <input data-testid="clone-user" type="text" autocomplete="username" bind:value={user} />
        </label>
        <label>
          Password or token
          <input data-testid="clone-password" type="password" autocomplete="current-password" bind:value={password} />
        </label>
      {/if}
      {#if error}<p class="error" role="alert" data-testid="clone-error">{error}</p>{/if}
      <div class="actions">
        <button type="button" data-testid="clone-cancel" onclick={onclose}>Cancel</button>
        <button type="submit" data-testid="clone-submit" disabled={!url.trim() || busy}>
          {busy ? "Cloning…" : "Clone"}
        </button>
      </div>
    </form>
  </dialog>
{/if}

<style>
  dialog {
    width: min(520px, calc(100vw - 32px));
    max-height: calc(100vh - 64px);
    box-sizing: border-box;
    padding: calc(var(--rt-gap) * 2);
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    font: inherit;
  }
  dialog::backdrop {
    background: var(--rt-bg-overlay);
  }
  form {
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
  }
  h2 {
    margin: 0;
    font-size: 1.1em;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
  }
  label input {
    padding: 3px 6px;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg);
    color: var(--rt-fg);
    font: inherit;
  }
  .error {
    margin: 0;
    color: var(--rt-danger);
    overflow-wrap: anywhere;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--rt-gap);
  }
</style>
