<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The short form of one kind in the "New…" dialog (wave 14, strand B). The name comes first
     and starts selected, so typing replaces the default; Enter in a field creates (the dialog's
     form submits), like Mod+Enter anywhere in the dialog. -->
<script lang="ts">
  import { getApp } from "../state/app.svelte";
  import { SegmentedControl, Select, TextField, type SelectOption } from "../ui";
  import { DEFAULT_ENVIRONMENT, HTTP_METHODS, URL_PLACEHOLDERS, withName, type FormErrors, type NewForm } from "./forms";
  import TargetFields from "./TargetFields.svelte";

  let {
    form = $bindable(),
    errors,
    desktop,
    roots,
  }: {
    form: NewForm;
    errors: FormErrors;
    desktop: boolean;
    /** Roots a collection can go into: `[id, name]`. */
    roots: [string, string][];
  } = $props();

  const app = getApp();

  /** Environments to copy from: those of the active collection (the only ones the shell lists). */
  const copyOptions = $derived<SelectOption[]>([
    { value: "", label: "Empty environment" },
    ...(form.kind === "environment" && form.collection === app.activeCollection
      ? app.environments.map((e) => ({ value: e.name, label: `Copy of ${e.name}` }))
      : []),
  ]);
  const methodOptions: SelectOption[] = HTTP_METHODS.map((m) => ({ value: m, label: m }));
  const rootOptions = $derived<SelectOption[]>(roots.map(([value, label]) => ({ value, label })));
  const rootName = $derived.by(() => {
    const f = form;
    return f.kind === "collection" ? (roots.find(([id]) => id === f.root)?.[1] ?? "") : "";
  });

  const NAME_LABELS: Record<string, string> = {
    collection: "Collection name",
    folder: "Folder name",
    environment: "Environment name",
    "mcp-test": "Test name",
  };

  // The name field starts focused with its default selected (the dialog's frame focuses
  // `data-autofocus` only when it opens, not when it moves from the tiles to a form).
  const focusName = (el: HTMLElement) => {
    const input = el.querySelector<HTMLInputElement>("[data-testid=new-name]");
    input?.focus();
    input?.select();
  };
</script>

<div class="fields">
  <div class="name" {@attach focusName}>
    <TextField
      label={NAME_LABELS[form.kind] ?? "Request name"}
      size="lg"
      value={form.name}
      oninput={(e) => (form = withName(form, e.currentTarget.value))}
      error={errors.name}
      autocomplete="off"
      spellcheck="false"
      data-testid="new-name"
      data-autofocus
    />
  </div>

  {#if form.kind === "http" || form.kind === "graphql" || form.kind === "grpc" || form.kind === "websocket" || form.kind === "sse"}
    {@const kind = form.kind}
    <div class="row">
      {#if kind === "http" && form.kind === "http"}
        <div class="method">
          <Select label="Method" options={methodOptions} bind:value={form.method} data-testid="new-method" />
        </div>
      {/if}
      <div class="grow">
        <TextField
          label={kind === "grpc" ? "Server" : "URL"}
          hint="Optional. Variables like {'{{baseUrl}}'} work here too."
          mono
          bind:value={form.url}
          placeholder={URL_PLACEHOLDERS[kind]}
          autocomplete="off"
          data-testid="new-url"
        />
      </div>
    </div>
    {#if kind === "sse"}
      <p class="note">Sent as <code>GET</code> with <code>Accept: text/event-stream</code>; events appear as they arrive.</p>
    {/if}
    <TargetFields bind:collection={form.collection} bind:folder={form.folder} error={errors.collection} />
  {:else if form.kind === "folder"}
    <TargetFields bind:collection={form.collection} bind:folder={form.folder} error={errors.collection} />
  {:else if form.kind === "environment"}
    <div class="row">
      <div class="grow"><TargetFields bind:collection={form.collection} folders={false} error={errors.collection} /></div>
      <div class="grow">
        <Select
          label="Start from"
          icon="copy"
          options={copyOptions}
          bind:value={() => (form.kind === "environment" ? (form.copyFrom ?? "") : ""), (v) => form.kind === "environment" && (form.copyFrom = v === "" ? null : v)}
          hint="Variables are copied; secret values never are."
          data-testid="new-copy-from"
        />
      </div>
    </div>
  {:else if form.kind === "collection"}
    <div class="group">
      <span class="caption" id="rt-new-place">Location</span>
      <SegmentedControl
        label="Location"
        options={[
          { value: "root", label: roots.length > 0 ? "New folder here" : "New folder here (open a folder first)", icon: "folder", disabled: roots.length === 0 },
          { value: "elsewhere", label: "Choose a folder…", icon: "folder-open" },
        ]}
        bind:value={form.place}
      />
      {#if form.place === "root"}
        <div class="row">
          {#if roots.length > 1}
            <div class="grow">
              <Select label="Inside" icon="folder-open" options={rootOptions} bind:value={() => (form.kind === "collection" ? (form.root ?? "") : ""), (v) => form.kind === "collection" && (form.root = v)} data-testid="new-root" />
            </div>
          {/if}
          <div class="grow">
            <TextField
              label="Folder"
              icon="folder"
              mono
              value={form.dir}
              oninput={(e) => (form = { ...form, dir: e.currentTarget.value, dirEdited: true } as NewForm)}
              hint={roots.length > 1 ? "Named after the collection until you change it." : `Inside ${rootName}; named after the collection until you change it.`}
              error={errors.dir}
              autocomplete="off"
              data-testid="new-dir"
            />
          </div>
        </div>
      {:else}
        <p class="note">After Create you choose the folder; it joins the sidebar as a new collection.</p>
      {/if}
    </div>
    <div class="row">
      <div class="grow">
        <TextField
          label="Base URL"
          icon="http"
          mono
          bind:value={form.baseUrl}
          placeholder="https://api.example.com"
          hint={form.environment.trim() ? `Optional. Becomes {{baseUrl}} in “${form.environment.trim()}”.` : "Optional. Becomes {{baseUrl}}."}
          autocomplete="off"
          data-testid="new-base-url"
        />
      </div>
      <div class="env">
        <TextField
          label="First environment"
          icon="environment"
          bind:value={form.environment}
          placeholder={DEFAULT_ENVIRONMENT}
          hint="Optional."
          error={errors.environment}
          autocomplete="off"
          data-testid="new-environment"
        />
      </div>
    </div>
  {:else if form.kind === "mcp-test"}
    <TargetFields bind:collection={form.collection} folders={false} error={errors.collection} />
    <div class="group">
      <span class="caption">Server</span>
      <SegmentedControl
        label="How to reach the server"
        options={[
          { value: "http", label: "Streamable HTTP", icon: "http" },
          { value: "stdio", label: "Local command", icon: "code", disabled: !desktop, title: desktop ? undefined : "Starting a local command needs the desktop app." },
        ]}
        bind:value={form.transport}
      />
      {#if form.transport === "http"}
        <TextField
          mono
          bind:value={form.url}
          placeholder="http://localhost:3001/mcp"
          aria-label="Server address"
          error={errors.url}
          hint={desktop ? undefined : "Local commands (stdio) need the desktop app."}
          autocomplete="off"
          data-testid="new-url"
        />
      {:else}
        <TextField
          mono
          bind:value={form.command}
          placeholder="npx -y @modelcontextprotocol/server-everything"
          aria-label="Command"
          error={errors.command}
          autocomplete="off"
          data-testid="new-command"
        />
      {/if}
    </div>
    <TextField
      label="Tool"
      icon="zap"
      mono
      bind:value={form.tool}
      placeholder="Pick it after connecting"
      hint="Optional. The MCP client opens next, where you connect and choose."
      autocomplete="off"
      data-testid="new-tool"
    />
  {/if}
</div>

<style>
  .fields {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-6);
  }
  .row {
    display: flex;
    align-items: flex-start;
    gap: var(--rt-space-5);
    min-width: 0;
  }
  .grow {
    flex: 1;
    min-width: 0;
  }
  .method {
    flex: none;
    width: 120px;
  }
  .env {
    flex: none;
    width: 200px;
  }
  .group {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--rt-space-4);
    min-width: 0;
  }
  .group > :global(*:not(.rt-segmented)) {
    align-self: stretch;
  }
  .caption {
    color: var(--rt-fg);
    font: var(--rt-weight-medium) var(--rt-text-small) / var(--rt-leading-small) var(--rt-font-ui);
  }
  .note {
    margin: 0;
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  code {
    font-family: var(--rt-font-mono);
    font-size: var(--rt-text-small);
  }
</style>
