<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- "Create variable" from the `{{` completion: name, value, where (environment, collection,
     folder) and, in an environment, secret yes/no. Shown in a CodeMirror tooltip next to the
     cursor; Enter creates, Esc or a click outside cancels. The value is stored unsaved like
     any edit; a secret value goes only to the secret store. -->
<script lang="ts">
  import { onMount } from "svelte";

  import { validName } from "./var-complete";
  import type { CreateKind, CreateTarget, NewVariable } from "./var-source";

  let {
    name: initialName,
    targets,
    onsubmit,
    oncancel,
  }: {
    name: string;
    targets: CreateTarget[];
    onsubmit: (kind: CreateKind, variable: NewVariable) => Promise<void>;
    oncancel: () => void;
  } = $props();

  // svelte-ignore state_referenced_locally
  let name = $state(initialName);
  let value = $state("");
  // svelte-ignore state_referenced_locally
  let kind = $state<CreateKind>(targets[0]?.kind ?? "collection");
  let secret = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let root: HTMLFormElement;
  let nameInput: HTMLInputElement;
  let valueInput: HTMLInputElement;

  const target = $derived(targets.find((t) => t.kind === kind) ?? targets[0]);
  const canSecret = $derived(target?.secretAllowed ?? false);
  const nameOk = $derived(validName(name));

  onMount(() => {
    (initialName && validName(initialName) ? valueInput : nameInput).focus();
    const outside = (e: PointerEvent) => {
      if (!root.contains(e.target as Node)) oncancel();
    };
    document.addEventListener("pointerdown", outside, true);
    root.addEventListener("keydown", keydown);
    return () => {
      document.removeEventListener("pointerdown", outside, true);
      root.removeEventListener("keydown", keydown);
    };
  });

  async function submit(e: Event) {
    e.preventDefault();
    if (!nameOk || busy || !target) return;
    busy = true;
    error = null;
    try {
      await onsubmit(target.kind, { name, value, secret: canSecret && secret });
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
      busy = false;
    }
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      oncancel();
    }
  }
</script>

<form class="rt-vc-form" data-testid="var-create-form" bind:this={root} onsubmit={submit}>
  <div class="title">Create variable</div>
  <label class="field">
    <span>Name</span>
    <input
      data-testid="var-create-name"
      bind:this={nameInput}
      bind:value={name}
      spellcheck="false"
      autocomplete="off"
      aria-invalid={name !== "" && !nameOk}
    />
  </label>
  <label class="field">
    <span>Value</span>
    <input
      data-testid="var-create-value"
      bind:this={valueInput}
      bind:value
      type={canSecret && secret ? "password" : "text"}
      spellcheck="false"
      autocomplete="off"
      placeholder={canSecret && secret ? "stored securely, never in files" : ""}
    />
  </label>
  <div class="field" role="radiogroup" aria-label="Where">
    <span>In</span>
    <div class="segments">
      {#each targets as t (t.kind)}
        <button
          type="button"
          role="radio"
          data-testid={`var-create-in-${t.kind}`}
          aria-checked={kind === t.kind}
          title={t.detail ? `${t.label} ${t.detail}` : t.label}
          onclick={() => (kind = t.kind)}
        >
          {t.label}{#if t.detail}<em>{t.detail}</em>{/if}
        </button>
      {/each}
    </div>
  </div>
  <label class="check" class:disabled={!canSecret} title={canSecret ? "" : "Secrets live in an environment"}>
    <input type="checkbox" data-testid="var-create-secret" bind:checked={secret} disabled={!canSecret} />
    Secret <small>{canSecret ? "value stays on this device" : "only in an environment"}</small>
  </label>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  <div class="actions">
    <button type="button" class="ghost" onclick={oncancel}>Cancel <kbd>Esc</kbd></button>
    <button type="submit" class="primary" data-testid="var-create-submit" disabled={!nameOk || busy}>
      Create <kbd>↵</kbd>
    </button>
  </div>
</form>

<style>
  .rt-vc-form {
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: min(320px, calc(100vw - 32px));
    padding: 12px;
    font-family: var(--rt-font-ui);
    font-size: var(--rt-size);
    color: var(--rt-fg);
  }
  .title {
    font-weight: 600;
  }
  .field {
    display: grid;
    grid-template-columns: 44px minmax(0, 1fr);
    align-items: center;
    gap: 8px;
  }
  .field > span {
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
  input:not([type="checkbox"]) {
    min-width: 0;
    padding: 4px 8px;
    font: inherit;
    font-family: var(--rt-font-mono);
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
  }
  input:focus-visible,
  button:focus-visible {
    outline: 2px solid var(--rt-focus);
    outline-offset: -1px;
  }
  input[aria-invalid="true"] {
    border-color: var(--rt-danger);
  }
  .segments {
    display: flex;
    min-width: 0;
    padding: 2px;
    gap: 2px;
    border: 1px solid var(--rt-border);
    border-radius: calc(var(--rt-radius) + 2px);
    background: var(--rt-bg-panel);
  }
  .segments button {
    flex: 1 1 0;
    min-width: 0;
    padding: 3px 6px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font: inherit;
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
    background: none;
    border: 0;
    border-radius: var(--rt-radius);
    cursor: pointer;
  }
  .segments button em {
    margin-left: 4px;
    font-style: normal;
    opacity: 0.8;
  }
  .segments button[aria-checked="true"] {
    color: var(--rt-fg);
    background: var(--rt-bg);
    box-shadow: 0 0 0 1px var(--rt-border);
  }
  .check {
    display: flex;
    align-items: center;
    gap: 6px;
    padding-left: 52px;
  }
  .check small {
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
  .check.disabled {
    color: var(--rt-fg-muted);
  }
  .error {
    margin: 0;
    color: var(--rt-danger);
    font-size: var(--rt-size-small);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
    margin-top: 2px;
  }
  .actions button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
    font: inherit;
    border-radius: var(--rt-radius);
    cursor: pointer;
  }
  .ghost {
    color: var(--rt-fg);
    background: none;
    border: 1px solid var(--rt-border);
  }
  .ghost:hover {
    background: var(--rt-bg-hover);
  }
  .primary {
    color: var(--rt-accent-fg);
    background: var(--rt-accent);
    border: 1px solid var(--rt-accent);
  }
  .primary:disabled {
    opacity: 0.5;
    cursor: default;
  }
  kbd {
    font-family: var(--rt-font-ui);
    font-size: 11px;
    opacity: 0.7;
  }
</style>
