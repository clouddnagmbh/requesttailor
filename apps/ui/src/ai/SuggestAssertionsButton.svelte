<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- "Suggest from response" in the assertions tab (wave 16, strand B): assertions for the
     request's current response, shown as a diff to apply or discard. Shown once the assistant
     is set up; disabled until the request has a response, or where the collection turned the
     assistant off. -->
<script lang="ts">
  import { getApp, type AppState } from "../state/app.svelte";
  import { aiFor } from "./ai.svelte";
  import { suggestAssertions } from "./flows";
  import { OFF_MESSAGE } from "./task";

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
  const title = $derived(
    ai?.off
      ? OFF_MESSAGE
      : response === null
        ? "Send the request first: the assistant suggests assertions from its response"
        : "Ask the AI assistant for assertions (secret values are masked)",
  );
</script>

{#if app && ai && (ai.ready || ai.off) && path}
  <div class="suggest">
    <button
      data-testid="ai-suggest-assertions"
      disabled={response === null || ai.off}
      {title}
      onclick={() => {
        if (response !== null) void suggestAssertions(app, ai, path, response);
      }}>Suggest from response</button
    >
    {#if ai.off}<span class="hint">{OFF_MESSAGE}</span>{:else if response === null}<span class="hint">Send the request first.</span>{/if}
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
