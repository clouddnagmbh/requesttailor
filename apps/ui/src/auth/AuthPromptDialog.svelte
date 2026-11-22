<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Shown while an OAuth 2.0 flow waits for the user (`backend.auth.onPrompt`): "continue in
     your browser" with the URL, or the device code to enter. Cancel (or Escape) stops the
     flow; `done` from the backend closes it. Owner: wave 5, strand C. -->
<script lang="ts">
  import type { AuthPrompt } from "../commands/types";
  import { getApp } from "../state/app.svelte";
  import { countdown } from "./format";

  const app = getApp();
  let prompt = $state<AuthPrompt | null>(null);
  /** When the current prompt arrived (for the device code countdown). */
  let since = $state(0);
  let now = $state(Date.now());
  let copied = $state(false);

  $effect(() =>
    app.backend.auth.onPrompt((p) => {
      prompt = p.kind === "done" ? null : p;
      since = Date.now();
      now = since;
      copied = false;
    }),
  );

  $effect(() => {
    if (prompt?.kind !== "device-code") return;
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });

  function cancel() {
    prompt = null;
    void app.backend.auth.cancelPrompt().catch(() => {});
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      copied = true;
    } catch {
      copied = false;
    }
  }

  function opened(node: HTMLDialogElement) {
    node.showModal();
    return () => node.close();
  }
</script>

{#if prompt}
  <dialog
    data-testid="auth-prompt"
    aria-labelledby="auth-prompt-title"
    aria-describedby="auth-prompt-text"
    {@attach opened}
    oncancel={(e) => {
      e.preventDefault();
      cancel();
    }}
  >
    <header>
      <h2 id="auth-prompt-title">{prompt.kind === "browser" ? "Sign in to get a token" : "Sign in on another device"}</h2>
    </header>
    <div class="body">
      {#if prompt.kind === "browser"}
        <p id="auth-prompt-text">
          Continue in your browser. RequestTailor continues as soon as the provider redirects back.
        </p>
        <div class="url">
          <code data-testid="auth-prompt-url">{prompt.url}</code>
        </div>
        <div class="actions">
          <a data-testid="auth-prompt-open" href={prompt.url} target="_blank" rel="noopener noreferrer">Open sign-in page</a>
          <button data-testid="auth-prompt-copy" onclick={() => prompt?.kind === "browser" && copy(prompt.url)}>
            {copied ? "Copied" : "Copy link"}
          </button>
        </div>
      {:else if prompt.kind === "device-code"}
        <p id="auth-prompt-text">Open the page below on any device and enter this code:</p>
        <p class="code" data-testid="auth-prompt-user-code">{prompt.user_code}</p>
        <p>
          <a data-testid="auth-prompt-verification" href={prompt.verification_uri} target="_blank" rel="noopener noreferrer"
            >{prompt.verification_uri}</a
          >
        </p>
        {#if prompt.verification_uri_complete}
          <p class="hint">
            Or open <a
              data-testid="auth-prompt-verification-complete"
              href={prompt.verification_uri_complete}
              target="_blank"
              rel="noopener noreferrer">the page with the code filled in</a
            >.
          </p>
        {/if}
        <div class="actions">
          <button data-testid="auth-prompt-copy" onclick={() => prompt?.kind === "device-code" && copy(prompt.user_code)}>
            {copied ? "Copied" : "Copy code"}
          </button>
          <span class="hint" data-testid="auth-prompt-expires" aria-live="off"
            >Expires in {countdown(prompt.expires_in, since, now)}</span
          >
        </div>
      {/if}
    </div>
    <footer>
      <button data-testid="auth-prompt-cancel" onclick={cancel}>Cancel</button>
    </footer>
  </dialog>
{/if}

<style>
  dialog {
    width: min(520px, calc(100vw - 32px));
    padding: 0;
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    font: inherit;
  }
  dialog::backdrop {
    background: var(--rt-bg-overlay);
  }
  header,
  footer {
    display: flex;
    align-items: center;
    gap: var(--rt-gap);
    padding: var(--rt-gap) calc(var(--rt-gap) * 2);
    background: var(--rt-bg-panel);
  }
  footer {
    justify-content: flex-end;
  }
  h2 {
    flex: 1;
    margin: 0;
    font-size: 1em;
  }
  .body {
    padding: calc(var(--rt-gap) * 2);
  }
  .body p {
    margin: 0 0 var(--rt-gap);
  }
  .url code {
    display: block;
    max-height: 6em;
    overflow: auto;
    word-break: break-all;
    font-family: var(--rt-font-mono, monospace);
    font-size: 0.9em;
    padding: 4px 6px;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg-panel);
  }
  .code {
    font-family: var(--rt-font-mono, monospace);
    font-size: 1.8em;
    letter-spacing: 0.12em;
    text-align: center;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: var(--rt-gap);
    margin-top: var(--rt-gap);
  }
  .hint {
    color: var(--rt-fg-muted);
  }
  button {
    font: inherit;
    color: var(--rt-fg);
    background: var(--rt-bg-panel);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    padding: 2px 8px;
    cursor: pointer;
  }
  button:hover {
    background: var(--rt-bg-hover);
  }
</style>
