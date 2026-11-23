<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- "Explain" in the response summary (wave 8, strand E; wave 16, strand B): once the assistant
     is set up. A failed response (error status, failed test or assertion) is explained as an
     error; any other response is summarised. Where the collection turned the assistant off,
     the button says so and does nothing. -->
<script lang="ts">
  import type { Backend } from "../commands/backend";
  import type { StoredResponse } from "../commands/types";
  import { getApp } from "../state/app.svelte";
  import { aiFor } from "./ai.svelte";
  import { failed } from "./apply";
  import { explainResponse } from "./flows";
  import { OFF_MESSAGE } from "./task";

  let { result, backend }: { result: StoredResponse; backend: Backend } = $props();

  // svelte-ignore state_referenced_locally
  const ai = aiFor(backend);
  void ai.ensure();
  const app = getApp();
  const path = $derived(result.report.request_path);
  const id = $derived(result.response_id);
  const bad = $derived(failed(result));
  const show = $derived((ai.ready || ai.off) && id !== null);

  function explain() {
    if (id === null || ai.off) return;
    if (bad) ai.open({ kind: "explain", path, response: id });
    else void explainResponse(app, ai, path, id);
  }
</script>

{#if show}
  <button
    class="explain"
    data-testid="ai-explain-button"
    disabled={ai.off}
    title={ai.off ? OFF_MESSAGE : bad ? "Ask the AI assistant why this failed (secret values are masked)" : "Ask the AI assistant what this response says (secret values are masked)"}
    onclick={explain}>Explain</button
  >
{/if}

<style>
  .explain {
    margin-left: auto;
    align-self: center;
    padding: 1px 8px;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg);
    color: var(--rt-fg);
    font: inherit;
    font-size: var(--rt-size-small);
    cursor: pointer;
  }
  .explain:hover:not(:disabled) {
    background: var(--rt-bg-hover);
  }
  .explain:disabled {
    opacity: 0.6;
    cursor: default;
  }
</style>
