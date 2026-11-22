<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- "Set as variable" (wave 7, strand C): takes a value of a JSON response into a session
     variable of the active environment, and optionally adds a `set-variable` action to the
     request so every later send takes it again. -->
<script lang="ts">
  import { getApp } from "../state/app.svelte";
  import { expressionFor, newAction, valueText, variableNameFor } from "./edit";
  import { sessionVars } from "./session.svelte";

  let {
    pick,
    requestPath,
    onclose,
  }: {
    /** The picked value and its tree path (`$.a.b`); `null`: closed. */
    pick: { path: string; value: unknown } | null;
    /** The request the response belongs to (for "also add as action"). */
    requestPath: string;
    onclose: () => void;
  } = $props();

  const app = getApp();
  const session = sessionVars(app);

  let name = $state("");
  let addAction = $state(false);
  let error = $state<string | null>(null);
  let busy = $state(false);

  const value = $derived(pick ? valueText(pick.value) : "");
  const expression = $derived(pick ? expressionFor(pick.path) : "");
  const canAddAction = $derived(app.drafts[requestPath] !== undefined);
  const validName = $derived(/^[^\s{}]+$/.test(name.trim()));

  $effect(() => {
    if (!pick) return;
    name = variableNameFor(pick.path);
    addAction = false;
    error = null;
  });

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (!pick || !validName || busy) return;
    busy = true;
    error = null;
    try {
      const variable = name.trim();
      await session.set({ name: variable, value, scope: "runtime" });
      const spec = app.drafts[requestPath];
      if (addAction && spec) {
        const next = $state.snapshot(spec);
        next.actions = [
          ...next.actions.filter((a) => !(a.variable === variable && a.expression === expression)),
          newAction(variable, expression),
        ];
        await app.edit(requestPath, next);
      }
      onclose();
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    } finally {
      busy = false;
    }
  }

  function opened(node: HTMLDialogElement) {
    node.showModal();
    return () => node.close();
  }
</script>

{#if pick}
  <dialog
    data-testid="set-variable-dialog"
    aria-labelledby="rt-set-var-title"
    {@attach opened}
    oncancel={(e) => {
      e.preventDefault();
      onclose();
    }}
  >
    <form onsubmit={submit}>
      <h2 id="rt-set-var-title">Set as variable</h2>
      <label>
        <span>Name</span>
        <!-- svelte-ignore a11y_autofocus -->
        <input data-testid="set-variable-name" bind:value={name} spellcheck="false" autocomplete="off" autofocus />
      </label>
      <div class="field">
        <span>Value</span>
        <code class="value" data-testid="set-variable-value" title={value}>{value}</code>
      </div>
      <p class="hint" data-testid="set-variable-scope">
        A session variable of {app.environment ? `the environment “${app.environment}”` : "no environment"}: used by the next sends
        until cleared, never written to a file.
      </p>
      {#if canAddAction}
        <label class="check">
          <input type="checkbox" data-testid="set-variable-action" bind:checked={addAction} />
          <span>Also add to this request: take <code>{expression}</code> after every send</span>
        </label>
      {/if}
      {#if error}<p class="error" role="alert">{error}</p>{/if}
      <div class="buttons">
        <button type="button" data-testid="set-variable-cancel" onclick={onclose}>Cancel</button>
        <button type="submit" class="primary" data-testid="set-variable-save" disabled={!validName || busy}>Set variable</button>
      </div>
    </form>
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
  form {
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
    padding: calc(var(--rt-gap) * 2);
  }
  h2 {
    margin: 0;
    font-size: 1em;
  }
  label,
  .field {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  label > span,
  .field > span {
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
  label.check {
    flex-direction: row;
    align-items: baseline;
    gap: var(--rt-gap);
  }
  label.check > span {
    color: var(--rt-fg);
    font-size: inherit;
  }
  input:not([type="checkbox"]) {
    font: inherit;
    font-family: var(--rt-font-mono);
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    padding: 2px 6px;
  }
  .value {
    display: block;
    max-height: 6em;
    overflow: auto;
    padding: 2px 6px;
    background: var(--rt-bg-panel);
    border-radius: var(--rt-radius);
    font-family: var(--rt-font-mono);
    white-space: pre-wrap;
    word-break: break-all;
  }
  .hint {
    margin: 0;
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
  .error {
    margin: 0;
    color: var(--rt-danger);
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: var(--rt-gap);
  }
  button {
    font: inherit;
    color: var(--rt-fg);
    background: var(--rt-bg-panel);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    padding: 2px 10px;
    cursor: pointer;
  }
  button:hover:not(:disabled) {
    background: var(--rt-bg-hover);
  }
  button:disabled {
    cursor: default;
    opacity: 0.6;
  }
  .primary {
    background: var(--rt-accent);
    color: var(--rt-accent-fg);
    border-color: var(--rt-accent);
  }
</style>
