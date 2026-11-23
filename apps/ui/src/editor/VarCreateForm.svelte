<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- "Create variable" from the `{{` completion: name, value, where (environment, collection,
     folder) and, in an environment, secret yes/no. Shown in a CodeMirror tooltip next to the
     cursor, in the look of a menu; Enter creates, Esc or a click outside cancels. The value is
     stored unsaved like any edit; a secret value goes only to the secret store. -->
<script lang="ts">
  import { onMount } from "svelte";

  import { Button, Checkbox, Icon, SegmentedControl, TextField } from "../ui";
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
  let kind = $state<string>(targets[0]?.kind ?? "collection");
  let secret = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let root: HTMLFormElement;
  let nameInput = $state<HTMLInputElement>();
  let valueInput = $state<HTMLInputElement>();

  const target = $derived(targets.find((t) => t.kind === kind) ?? targets[0]);
  const canSecret = $derived(target?.secretAllowed ?? false);
  const nameOk = $derived(validName(name));
  const options = $derived(
    targets.map((t) => ({
      value: t.kind,
      label: t.kind === "environment" ? (t.detail ?? t.label) : t.kind === "folder" ? (t.detail ?? t.label) : t.label,
      icon: t.kind,
      title: t.detail ? `${t.label} ${t.detail}` : t.label,
    })),
  );

  onMount(() => {
    (initialName && validName(initialName) ? valueInput : nameInput)?.focus();
    const outside = (e: PointerEvent) => {
      if (!root.contains(e.target as Node)) oncancel();
    };
    const keydown = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      oncancel();
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
</script>

<form class="rt-vc-form" data-testid="var-create-form" bind:this={root} onsubmit={submit}>
  <header>
    <Icon name="variable" size="sm" />
    <span>Create variable</span>
  </header>
  <TextField
    label="Name"
    size="sm"
    mono
    bind:value={name}
    bind:element={nameInput}
    autocomplete="off"
    data-testid="var-create-name"
    error={name !== "" && !nameOk ? "No braces or surrounding spaces." : undefined}
  />
  <TextField
    label="Value"
    size="sm"
    mono
    bind:value
    bind:element={valueInput}
    type={canSecret && secret ? "password" : "text"}
    autocomplete="off"
    placeholder={canSecret && secret ? "Stays on this device, never in files" : ""}
    data-testid="var-create-value"
  />
  <div class="where">
    <span class="label">Store in</span>
    <SegmentedControl label="Store in" size="sm" {options} bind:value={kind} />
  </div>
  <div title={canSecret ? undefined : "Secrets live in an environment"}>
    <Checkbox
      label="Secret"
      description={canSecret ? "Value in the keychain, only the name in files" : "Only in an environment"}
      bind:checked={secret}
      disabled={!canSecret}
      data-testid="var-create-secret"
    />
  </div>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  <footer>
    <span class="spacer"></span>
    <Button size="sm" variant="ghost" onclick={oncancel}>Cancel</Button>
    <Button size="sm" variant="primary" type="submit" loading={busy} disabled={!nameOk} data-testid="var-create-submit">
      Create
    </Button>
  </footer>
</form>

<style>
  .rt-vc-form {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-5);
    box-sizing: border-box;
    width: min(340px, calc(100vw - 32px));
    padding: var(--rt-space-5) var(--rt-space-5) var(--rt-space-4);
    font: var(--rt-font-body);
    color: var(--rt-fg);
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--rt-space-3);
    font: var(--rt-font-title);
  }
  header :global(.rt-icon) {
    color: var(--rt-accent);
  }
  .where {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-2);
  }
  .label {
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .error {
    margin: 0;
    color: var(--rt-danger-fg);
    font: var(--rt-font-small);
  }
  footer {
    display: flex;
    align-items: center;
    gap: var(--rt-space-3);
    margin: 0 calc(-1 * var(--rt-space-5));
    padding: var(--rt-space-4) var(--rt-space-5) 0;
    border-top: 1px solid var(--rt-border);
  }
  .spacer {
    flex: 1;
  }
</style>
