<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- "Explain" for a failed response (wave 8, strand E): asks once on opening and shows the
     answer. Nothing changes in the collection. -->
<script lang="ts">
  import type { AppState } from "../state/app.svelte";
  import type { AiState } from "./ai.svelte";
  import { AiDialog } from "@tailor-kit/ai-ui";
  import { askError } from "./apply";
  import { Markdown } from "@tailor-kit/ai-ui";

  let { ai, app, path, response }: { ai: AiState; app: AppState; path: string; response: number } = $props();

  let busy = $state(true);
  let text = $state<string | null>(null);
  let error = $state<string | null>(null);

  async function ask() {
    busy = true;
    error = null;
    try {
      text = (await ai.backend.ai.ask({ task: "explain-error", path, response }, { environment: app.environment, vars: [] })).text;
    } catch (e) {
      error = askError(e);
    } finally {
      busy = false;
    }
  }
  void ask();
</script>

<AiDialog title="Why did this fail?" testid="ai-explain" busy={false} onclose={() => ai.close()}>
  <p class="muted">{path}</p>
  {#if busy}
    <p data-testid="ai-explain-busy" aria-live="polite">Asking the model…</p>
  {:else if error}
    <p class="error" role="alert" data-testid="ai-explain-error">{error}</p>
  {:else if text}
    <Markdown {text} testid="ai-explain-text" />
  {/if}
  <div class="row">
    {#if error}<button data-testid="ai-explain-retry" onclick={ask}>Try again</button>{/if}
    <button data-testid="ai-explain-close" onclick={() => ai.close()}>Close</button>
  </div>
</AiDialog>
