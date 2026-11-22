<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- "Suggest tests" (wave 8, strand E): asks once on opening, shows the proposed tests
     script and assertions; "Add to request" appends them as unsaved edits. -->
<script lang="ts">
  import type { AiAnswer } from "../commands/types";
  import type { AppState } from "../state/app.svelte";
  import type { AiState } from "./ai.svelte";
  import AiDialog from "./AiDialog.svelte";
  import { askError, withSuggestedTests } from "./apply";
  import Markdown from "./Markdown.svelte";

  let { ai, app, path, response }: { ai: AiState; app: AppState; path: string; response: number } = $props();

  let busy = $state(true);
  let answer = $state.raw<AiAnswer | null>(null);
  let error = $state<string | null>(null);

  async function ask() {
    busy = true;
    error = null;
    try {
      answer = await ai.backend.ai.ask({ task: "suggest-tests", path, response }, { environment: app.environment, vars: [] });
    } catch (e) {
      error = askError(e);
    } finally {
      busy = false;
    }
  }
  void ask();

  async function apply() {
    if (!answer) return;
    busy = true;
    try {
      const spec = app.drafts[path] ?? (await app.backend.getRequest(path));
      await app.edit(path, withSuggestedTests($state.snapshot(spec), answer));
      ai.close();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  const tests = $derived(answer?.scripts?.filter((s) => s.phase === "tests").map((s) => s.code).join("\n\n") ?? "");
</script>

<AiDialog title="Suggested tests" testid="ai-tests" busy={false} onclose={() => ai.close()}>
  <p class="muted">{path}</p>
  {#if busy && !answer}
    <p data-testid="ai-tests-busy" aria-live="polite">Asking the model…</p>
  {:else if answer}
    {#if answer.text}<Markdown text={answer.text} testid="ai-tests-text" />{/if}
    {#if tests}
      <p><strong>Tests script</strong> <span class="muted">(added after the existing one)</span></p>
      <pre data-testid="ai-tests-script">{tests}</pre>
    {/if}
    {#if (answer.assertions ?? []).length > 0}
      <p><strong>Assertions</strong></p>
      <ul data-testid="ai-tests-assertions">
        {#each answer.assertions ?? [] as a, i (i)}
          <li><code>{a.expression} {a.operator} {a.value}</code>{#if a.description}<span class="muted"> — {a.description}</span>{/if}</li>
        {/each}
      </ul>
    {/if}
  {/if}
  {#if error}<p class="error" role="alert" data-testid="ai-tests-error">{error}</p>{/if}
  <div class="row">
    {#if answer}
      <button class="primary" data-testid="ai-tests-apply" disabled={busy} onclick={apply}>Add to request</button>
    {:else if error}
      <button data-testid="ai-tests-retry" onclick={ask}>Try again</button>
    {/if}
    <button data-testid="ai-tests-close" disabled={busy && answer !== null} onclick={() => ai.close()}>Close</button>
  </div>
</AiDialog>

<style>
  ul {
    margin: 0;
    padding-left: 1.4em;
  }
  code {
    font-family: var(--rt-font-mono);
    font-size: var(--rt-size-small);
  }
</style>
