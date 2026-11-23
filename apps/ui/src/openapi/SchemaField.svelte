<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- One input of the run wizard, its control chosen from the JSON Schema (`form.ts`): enums as
     a Select, booleans as true/false, numbers, dates, UUIDs and emails with "Generate", JSON for
     objects and arrays; the description as help, the example as one-click fill, the validation
     message under the field as the user types. -->
<script lang="ts">
  import type { Snippet } from "svelte";

  import { Button, Icon, Select, TextArea, TextField } from "../ui";
  import { describe, generate, validateText } from "./form";

  let {
    id,
    name,
    required = false,
    schema,
    description = null,
    example = null,
    secret = false,
    value = $bindable(""),
    /** A problem the core reported (shown until the user edits). */
    reported = null,
    touched = $bindable(false),
    tag,
    extra,
  }: {
    id: string;
    name: string;
    required?: boolean;
    schema: unknown;
    description?: string | null;
    example?: string | null;
    secret?: boolean;
    value?: string;
    reported?: string | null;
    touched?: boolean;
    /** Small label after the name (`path`, `query`, …). */
    tag?: string;
    /** Controls at the end of the label row (the variable's layer). */
    extra?: Snippet;
  } = $props();

  const model = $derived(describe(schema));
  const live = $derived(validateText(schema, value, required));
  // Messages from the core quote names in backticks; the field shows them plain.
  const plain = (m: string | null | undefined) => (m ? m.replace(/`([^`]+)`/g, "$1") : undefined);
  const error = $derived(touched ? plain(live) : plain(reported));
  const hint = $derived([description?.split("\n")[0], model.constraints].filter(Boolean).join(" · ") || undefined);
  const exampleText = $derived(example && example !== value && example.length <= 200 ? example : null);
  const control = $derived(`${id}-control`);

  function set(v: string) {
    value = v;
    touched = true;
  }
</script>

<div class="field" data-testid="ready-field" data-field={id}>
  <div class="label-row">
    <label for={control} class="name">
      <code>{name}</code>
      {#if required}<span class="req" aria-label="required">*</span>{/if}
      {#if tag}<span class="tag">{tag}</span>{/if}
      {#if secret}<span class="secret"><Icon name="lock" size="sm" /> Keychain</span>{/if}
    </label>
    <span class="spacer"></span>
    {#if extra}{@render extra()}{/if}
  </div>
  <div class="control-row">
    <div class="control">
      {#if model.kind === "enum" || model.kind === "boolean"}
        <Select
          id={control}
          value={value}
          options={[{ value: "", label: required ? "Choose…" : "(not set)" }, ...model.options.map((o) => ({ value: o, label: o }))]}
          error={error}
          hint={hint}
          onchange={(e) => set((e.currentTarget as HTMLSelectElement).value)}
        />
      {:else if model.kind === "json"}
        <TextArea id={control} bind:value rows={4} mono error={error} hint={hint} placeholder={model.placeholder} oninput={() => (touched = true)} />
      {:else}
        <TextField
          id={control}
          bind:value
          mono={model.kind !== "text" || secret}
          type={secret ? "password" : model.kind === "integer" || model.kind === "number" ? "text" : "text"}
          inputmode={model.kind === "integer" ? "numeric" : model.kind === "number" ? "decimal" : undefined}
          placeholder={model.placeholder}
          autocomplete="off"
          spellcheck={false}
          error={error}
          hint={hint}
          oninput={() => (touched = true)}
        />
      {/if}
    </div>
    {#if model.generatable}
      <Button size="sm" variant="ghost" icon="generate" onclick={() => set(generate(schema))} data-testid="ready-generate">Generate</Button>
    {/if}
  </div>
  {#if exampleText}
    <div class="example">
      <Button size="sm" variant="ghost" onclick={() => set(exampleText)} data-testid="ready-example">Use example</Button>
      <code title={exampleText}>{exampleText}</code>
    </div>
  {/if}
</div>

<style>
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-2);
  }
  .label-row {
    display: flex;
    align-items: center;
    gap: var(--rt-space-3);
    min-height: var(--rt-control-sm);
  }
  .name {
    display: flex;
    align-items: center;
    gap: var(--rt-space-2);
    font-weight: 600;
  }
  .name code {
    font-family: var(--rt-font-mono);
    font-size: 12.5px;
  }
  .req {
    color: var(--rt-danger-fg);
  }
  .tag {
    padding: 0 var(--rt-space-2);
    border-radius: var(--rt-radius-sm);
    background: var(--rt-surface-sunken);
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
    font-weight: 400;
  }
  .secret {
    display: inline-flex;
    align-items: center;
    gap: var(--rt-space-1);
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
    font-weight: 400;
  }
  .spacer {
    flex: 1;
  }
  .control-row {
    display: flex;
    align-items: flex-start;
    gap: var(--rt-space-3);
  }
  .control {
    flex: 1;
    min-width: 0;
  }
  .example {
    display: flex;
    align-items: center;
    gap: var(--rt-space-2);
    min-width: 0;
    margin-left: calc(-1 * var(--rt-space-3));
  }
  .example code {
    overflow: hidden;
    color: var(--rt-fg-muted);
    font-family: var(--rt-font-mono);
    font-size: 12px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
