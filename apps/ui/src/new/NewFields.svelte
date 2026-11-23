<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The short form of one kind in the "New…" dialog (wave 14, strand B). The name comes first
     and starts selected, so typing replaces the default; Enter in a field creates (the dialog's
     form submits), like Mod+Enter anywhere in the dialog. -->
<script lang="ts">
  import { getApp } from "../state/app.svelte";
  import {
    DEFAULT_ENVIRONMENT,
    HTTP_METHODS,
    URL_PLACEHOLDERS,
    withName,
    type FormErrors,
    type NewForm,
  } from "./forms";
  import TargetFields from "./TargetFields.svelte";

  let {
    form = $bindable(),
    errors,
    desktop,
    pickRoot,
  }: {
    form: NewForm;
    errors: FormErrors;
    desktop: boolean;
    /** Roots a collection can go into: `[id, name]`. */
    pickRoot: [string, string][];
  } = $props();

  const app = getApp();

  /** Environments to copy from: those of the active collection (the only ones the shell lists). */
  const environments = $derived(
    form.kind === "environment" && form.collection === app.activeCollection ? app.environments.map((e) => e.name) : [],
  );

  const focusName = (el: HTMLInputElement) => {
    el.focus();
    el.select();
  };

  function setName(e: Event) {
    form = withName(form, (e.currentTarget as HTMLInputElement).value);
  }

  const nameLabel = $derived(
    form.kind === "collection" ? "Collection name" : form.kind === "folder" ? "Folder name" : form.kind === "environment" ? "Environment name" : form.kind === "mcp-test" ? "Test name" : "Request name",
  );
</script>

<div class="fields">
  <label class="field">
    <span class="label">{nameLabel}</span>
    <input
      data-testid="new-name"
      data-autofocus
      type="text"
      value={form.name}
      oninput={setName}
      aria-invalid={errors.name ? "true" : undefined}
      autocomplete="off"
      spellcheck="false"
      {@attach focusName}
    />
    {#if errors.name}<span class="error" role="alert">{errors.name}</span>{/if}
  </label>

  {#if form.kind === "http" || form.kind === "graphql" || form.kind === "grpc" || form.kind === "websocket" || form.kind === "sse"}
    <div class="url-row">
      {#if form.kind === "http"}
        <label class="field method">
          <span class="label">Method</span>
          <select data-testid="new-method" bind:value={form.method}>
            {#each HTTP_METHODS as m (m)}<option value={m}>{m}</option>{/each}
          </select>
        </label>
      {/if}
      <label class="field grow">
        <span class="label">{form.kind === "grpc" ? "Server" : "URL"} <span class="optional">optional</span></span>
        <input data-testid="new-url" type="text" bind:value={form.url} placeholder={URL_PLACEHOLDERS[form.kind]} autocomplete="off" spellcheck="false" />
      </label>
    </div>
    {#if form.kind === "sse"}<p class="note">Sent as GET with <code>Accept: text/event-stream</code>; events show as they arrive.</p>{/if}
    <TargetFields bind:collection={form.collection} bind:folder={form.folder} error={errors.collection} />
  {:else if form.kind === "folder"}
    <TargetFields bind:collection={form.collection} bind:folder={form.folder} error={errors.collection} />
  {:else if form.kind === "environment"}
    <TargetFields bind:collection={form.collection} folders={false} error={errors.collection} />
    <label class="field">
      <span class="label">Start from</span>
      <select data-testid="new-copy-from" bind:value={form.copyFrom}>
        <option value={null}>Empty environment</option>
        {#each environments as name (name)}<option value={name}>Copy of {name}</option>{/each}
      </select>
      <span class="hint">Variables are copied; secret values never are. Set them after creating.</span>
    </label>
  {:else if form.kind === "collection"}
    <fieldset class="field">
      <legend class="label">Location</legend>
      {#if pickRoot.length > 0}
        <label class="choice">
          <input type="radio" name="new-place" value="root" bind:group={form.place} data-testid="new-place-root" />
          <span>In a new folder of</span>
          <select data-testid="new-root" bind:value={form.root} disabled={form.place !== "root"} aria-label="Folder to create it in">
            {#each pickRoot as [id, name] (id)}<option value={id}>{name}</option>{/each}
          </select>
        </label>
        {#if form.place === "root"}
          <label class="field indent">
            <span class="label">Folder</span>
            <input
              data-testid="new-dir"
              type="text"
              value={form.dir}
              oninput={(e) => (form = { ...form, dir: e.currentTarget.value, dirEdited: true } as NewForm)}
              aria-invalid={errors.dir ? "true" : undefined}
              autocomplete="off"
              spellcheck="false"
            />
            {#if errors.dir}<span class="error" role="alert">{errors.dir}</span>{/if}
          </label>
        {/if}
      {/if}
      <label class="choice">
        <input type="radio" name="new-place" value="elsewhere" bind:group={form.place} data-testid="new-place-elsewhere" />
        <span>Elsewhere… <span class="hint">choose a folder after Create</span></span>
      </label>
    </fieldset>
    <div class="url-row">
      <label class="field grow">
        <span class="label">Base URL <span class="optional">optional</span></span>
        <input data-testid="new-base-url" type="text" bind:value={form.baseUrl} placeholder="https://api.example.com" autocomplete="off" spellcheck="false" />
      </label>
      <label class="field">
        <span class="label">First environment <span class="optional">optional</span></span>
        <input data-testid="new-environment" type="text" bind:value={form.environment} placeholder={DEFAULT_ENVIRONMENT} autocomplete="off" />
        {#if errors.environment}<span class="error" role="alert">{errors.environment}</span>{/if}
      </label>
    </div>
    <p class="note">The base URL becomes the variable <code>{"{{baseUrl}}"}</code>{form.environment.trim() ? ` of “${form.environment.trim()}”` : ""}.</p>
  {:else if form.kind === "mcp-test"}
    <TargetFields bind:collection={form.collection} folders={false} error={errors.collection} />
    <fieldset class="field">
      <legend class="label">Server</legend>
      <div class="segments" role="radiogroup" aria-label="How to reach the server">
        <label class="choice"><input type="radio" value="http" bind:group={form.transport} data-testid="new-transport-http" /> Streamable HTTP</label>
        <label class="choice" title={desktop ? "" : "Starting a local command needs the desktop app."}>
          <input type="radio" value="stdio" bind:group={form.transport} disabled={!desktop} data-testid="new-transport-stdio" /> Local command (stdio)
        </label>
      </div>
      {#if form.transport === "http"}
        <input data-testid="new-url" type="text" bind:value={form.url} placeholder="http://localhost:3001/mcp" aria-label="Server address" autocomplete="off" spellcheck="false" />
        {#if errors.url}<span class="error" role="alert">{errors.url}</span>{/if}
      {:else}
        <input data-testid="new-command" type="text" bind:value={form.command} placeholder="npx -y @modelcontextprotocol/server-everything" aria-label="Command" autocomplete="off" spellcheck="false" />
        {#if errors.command}<span class="error" role="alert">{errors.command}</span>{/if}
      {/if}
    </fieldset>
    <label class="field">
      <span class="label">Tool <span class="optional">optional</span></span>
      <input data-testid="new-tool" type="text" bind:value={form.tool} placeholder="Pick it after connecting" autocomplete="off" spellcheck="false" />
    </label>
  {/if}
</div>

<style>
  .fields {
    display: flex;
    flex-direction: column;
    gap: calc(var(--rt-gap) * 1.5);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
    margin: 0;
    padding: 0;
    border: 0;
  }
  .label {
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
  }
  .optional,
  .hint {
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
  .optional {
    opacity: 0.8;
  }
  .url-row {
    display: flex;
    gap: var(--rt-gap);
    min-width: 0;
  }
  .grow {
    flex: 1;
  }
  .method {
    flex: none;
  }
  .choice {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .indent {
    padding-left: 22px;
  }
  .segments {
    display: flex;
    flex-wrap: wrap;
    gap: var(--rt-gap) calc(var(--rt-gap) * 2);
  }
  .note {
    margin: 0;
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
  .error {
    color: var(--rt-danger);
    font-size: var(--rt-size-small);
  }
</style>
