<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- "Explain" in the response summary (wave 8, strand E): only for a failed response (error
     status, failed test or assertion) and only once the assistant is set up. -->
<script lang="ts">
  import type { Backend } from "../commands/backend";
  import type { StoredResponse } from "../commands/types";
  import { aiFor } from "./ai.svelte";
  import { failed } from "./apply";

  let { result, backend }: { result: StoredResponse; backend: Backend } = $props();

  // svelte-ignore state_referenced_locally
  const ai = aiFor(backend);
  void ai.ensure();
  const show = $derived(ai.configured && failed(result));
</script>

{#if show}
  <button
    class="explain"
    data-testid="ai-explain-button"
    title="Ask the AI assistant why this failed (secret values are masked)"
    onclick={() => ai.open({ kind: "explain", path: result.report.request_path, response: result.response_id! })}
    >Explain</button
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
  .explain:hover {
    background: var(--rt-bg-hover);
  }
</style>
