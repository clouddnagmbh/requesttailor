<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- "Suggest tests" in the scripts tab (wave 8, strand E): shown once the assistant is set up,
     enabled after the request has a response (the model sees it, masked). -->
<script lang="ts">
  import { getApp, type AppState } from "../state/app.svelte";
  import { aiFor } from "./ai.svelte";

  // Outside the shell (component tests) there is no app and no button.
  function appOrNull(): AppState | null {
    try {
      return getApp();
    } catch {
      return null;
    }
  }
  const app = appOrNull();
  const ai = app ? aiFor(app.backend) : null;
  void ai?.ensure();
  const path = $derived(app?.active ?? null);
  const response = $derived(path ? (app?.results[path]?.response_id ?? null) : null);
</script>

{#if app && ai?.configured && path}
  <div class="suggest">
    <button
      data-testid="ai-suggest-tests"
      disabled={response === null}
      title={response === null ? "Send the request first: the assistant suggests tests from its response" : "Ask the AI assistant for tests (secret values are masked)"}
      onclick={() => {
        if (response !== null) ai.open({ kind: "tests", path, response });
      }}>Suggest tests with AI</button
    >
    {#if response === null}<span class="hint">Send the request first.</span>{/if}
  </div>
{/if}

<style>
  .suggest {
    display: flex;
    align-items: center;
    gap: var(--rt-gap);
  }
  button {
    padding: 2px 10px;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg);
    color: var(--rt-fg);
    font: inherit;
    font-size: var(--rt-size-small);
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .hint {
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
</style>
