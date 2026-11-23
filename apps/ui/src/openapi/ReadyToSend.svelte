<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- "Ready to send?": what the request still needs for its spec operation (`OpenApiApi.
     requirements`), as a form generated from the schema. "Fill and send" applies the values like
     any edit (unsaved): parameters and the body to the request, variables to the environment
     (default for baseUrl and auth) or the request, secrets only to the keychain; then sends.
     "Send anyway" and "Don't ask for this request" are always there; Mod+Enter = Fill and send. -->
<script lang="ts">
  import type { InputField, SendRequirements } from "../commands/types";
  import type { AppState } from "../state/app.svelte";
  import { Banner, Button, Checkbox, DialogFooter, Kbd, Section, SegmentedControl, Select, TextField } from "../ui";
  import { initialState, planApply, type FormState, type VarLayer } from "./apply";
  import { bodyFromForm, flatObject, formFromBody, sample, validateJson, validateText } from "./form";
  import type { GateController, GateResult } from "./gate";
  import { setSkipped } from "./gate";
  import JsonSchemaEditor from "./JsonSchemaEditor.svelte";
  import SchemaField from "./SchemaField.svelte";

  let {
    app,
    path,
    requirements,
    controller,
    done,
  }: {
    app: AppState;
    path: string;
    requirements: SendRequirements;
    controller: GateController;
    done: (result: GateResult) => void;
  } = $props();

  const NEW_ENV = "\u0000new";
  // svelte-ignore state_referenced_locally
  const req = requirements;
  // The environment at the moment the panel opens.
  // svelte-ignore state_referenced_locally
  const startEnv = app.environment ?? app.environments[0]?.name ?? null;
  let form = $state<FormState>(initialState(req, startEnv));
  let envChoice = $state(startEnv ?? NEW_ENV);
  let newEnvName = $state(startEnv === null ? "Default" : "");
  // Every key a field binds to exists from the start (bound props with fallbacks need values).
  const startBody = req.fields.find((f) => f.place === "body") ?? null;
  const startFlat = startBody ? flatObject(startBody.schema) : null;
  const startBodyValues = startBody && startFlat ? formFromBody(startBody.value, startFlat) : null;
  let touched = $state<Record<string, boolean>>(
    Object.fromEntries([...req.fields.map((f) => f.id), ...(startFlat ?? []).map((p) => `body.${p.name}`), "environment"].map((k) => [k, false])),
  );
  let dontAsk = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let bodyMode = $state<"form" | "json">(startFlat && startBodyValues ? "form" : "json");
  let bodyValues = $state<Record<string, string>>(startBodyValues ?? Object.fromEntries((startFlat ?? []).map((p) => [p.name, ""])));
  let baseUrlMode = $state<"server" | "custom">("server");

  const problems = $derived(req.fields.filter((f) => f.problem));
  const others = $derived(req.fields.filter((f) => !f.problem && f.place !== "variable"));
  const bodyField = $derived(req.fields.find((f) => f.place === "body") ?? null);
  const flat = $derived(bodyField ? flatObject(bodyField.schema) : null);
  const environmentName = $derived(envChoice === NEW_ENV ? newEnvName.trim() : envChoice);
  const needsEnvironment = $derived(
    req.fields.some((f) => f.place === "variable" && (f.secret || form.layers[f.id] === "environment")),
  );

  const PLACE_LABEL: Record<InputField["place"], string> = {
    variable: "variable",
    path: "path",
    query: "query",
    header: "header",
    body: "body",
  };

  const intro = $derived.by(() => {
    const missing = problems.filter((f) => f.problem?.kind === "missing").length;
    const invalid = problems.length - missing;
    const parts = [
      missing && `${missing} required ${missing === 1 ? "value is" : "values are"} missing`,
      invalid && `${invalid} ${invalid === 1 ? "value does" : "values do"} not match the spec`,
    ].filter(Boolean);
    const text = parts.join(", and ");
    return text.charAt(0).toUpperCase() + text.slice(1) + (problems.length === 1 ? ". Fill it in here, or send as is." : ". Fill them in here, or send as is.");
  });

  function bodyText(): string {
    if (!bodyField) return "";
    if (bodyMode === "form" && flat) return bodyFromForm(flat, bodyValues);
    return form.values[bodyField.id] ?? "";
  }

  /** Messages that still stop "Fill and send", by field id. */
  function errors(): Record<string, string> {
    const out: Record<string, string> = {};
    for (const f of req.fields) {
      if (f.place === "body") {
        const text = bodyText();
        if (text.trim() === "") {
          if (f.required) out[f.id] = "a request body is required";
          continue;
        }
        try {
          const first = validateJson(f.schema, JSON.parse(text))[0];
          if (first) out[f.id] = `${first.path}: ${first.message}`;
        } catch {
          if (!/\{\{[^}]+\}\}/.test(text)) out[f.id] = "the body is not valid JSON";
        }
        continue;
      }
      const value = form.values[f.id] ?? "";
      if (f.secret && value === "" && f.problem) {
        out[f.id] = "required";
        continue;
      }
      const message = validateText(f.schema, value, f.required || !!f.problem);
      if (message) out[f.id] = message;
    }
    if (needsEnvironment && environmentName === "") out.environment = "Name the environment the values go to.";
    return out;
  }

  async function fill() {
    if (busy) return;
    const errs = errors();
    if (Object.keys(errs).length) {
      touched = Object.fromEntries(Object.keys(touched).map((k) => [k, true]));
      error = "Some values are still missing or not valid. Fix them, or send anyway.";
      return;
    }
    busy = true;
    error = null;
    try {
      const state: FormState = { ...$state.snapshot(form), environment: environmentName || null };
      if (bodyField) state.values[bodyField.id] = bodyText();
      const draft = app.drafts[path] ?? (await app.backend.getRequest(path));
      const plan = planApply($state.snapshot(draft) as typeof draft, req, state, app.environments);
      if (plan.blocked) throw new Error(plan.blocked);
      if (plan.changed) await app.edit(path, plan.spec);
      if (plan.environment) {
        await app.backend.updateEnvironment(plan.environment);
        for (const s of plan.secrets) await app.backend.secrets.set(s.environment, s.name, s.value);
        await app.reloadEnvironments();
        app.setEnvironment(plan.environment.name);
      }
      await app.settled();
      finish("send");
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  function finish(result: GateResult) {
    if (dontAsk && result === "send" && app.collection) setSkipped(app.collection.id, path, true);
    done(result);
  }

  // svelte-ignore state_referenced_locally
  controller.submit = () => void fill();

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      void fill();
    }
  }

  const layerOptions = [
    { value: "environment", label: "Environment" },
    { value: "request", label: "This request" },
  ];
  const envOptions = $derived([
    ...app.environments.map((e) => ({ value: e.name, label: e.name })),
    { value: NEW_ENV, label: "New environment…" },
  ]);
  const serverOptions = $derived(req.servers.map((s) => ({ value: s.url, label: s.description ? `${s.description} — ${s.url}` : s.url })));
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="ready" data-testid="ready-to-send" {onkeydown}>
  <p class="intro">
    <span>{intro}</span>
    {#if req.summary}<span class="muted">{req.summary}</span>{/if}
  </p>
  {#if req.deprecated}<Banner tone="warn">The spec marks this operation as deprecated.</Banner>{/if}

  {#if needsEnvironment || req.fields.some((f) => f.place === "variable")}
    <div class="env-row">
      <Select label="Environment" bind:value={envChoice} options={envOptions} size="sm" data-testid="ready-environment" />
      {#if envChoice === NEW_ENV}
        <TextField label="Name" bind:value={newEnvName} size="sm" placeholder="Production" error={touched.environment ? errors().environment : undefined} data-testid="ready-new-env" />
      {/if}
    </div>
  {/if}

  <div class="fields">
    {#each problems as f (f.id)}
      {#if f.place === "body"}
        {@render body(f)}
      {:else if f.name === "baseUrl" && req.servers.length > 0}
        <div class="baseurl" data-testid="ready-field" data-field={f.id}>
          <div class="baseurl-head">
            <code class="strong">baseUrl</code><span class="req">*</span>
            <span class="muted small">{f.problem?.message}</span>
            <span class="spacer"></span>
            <SegmentedControl
              size="sm"
              label="baseUrl from"
              options={[
                { value: "server", label: "Spec server" },
                { value: "custom", label: "Custom" },
              ]}
              bind:value={baseUrlMode}
            />
          </div>
          {#if baseUrlMode === "server"}
            <Select
              aria-label="Server"
              value={form.values[f.id] ?? ""}
              options={[{ value: "", label: "Choose a server…" }, ...serverOptions]}
              onchange={(e) => (form.values[f.id] = (e.currentTarget as HTMLSelectElement).value)}
              data-testid="ready-server"
            />
          {:else}
            <TextField bind:value={form.values[f.id]} mono placeholder="https://api.example.com" aria-label="baseUrl" />
          {/if}
          {@render layer(f)}
        </div>
      {:else}
        <SchemaField
          id={f.id}
          name={f.name}
          required={f.required || f.place === "variable"}
          schema={f.schema}
          description={f.description}
          example={f.secret ? null : f.example}
          secret={f.secret}
          tag={PLACE_LABEL[f.place]}
          bind:value={form.values[f.id]}
          bind:touched={touched[f.id]}
          reported={f.problem?.kind === "invalid" ? f.problem.message : null}
        >
          {#snippet extra()}{@render layer(f)}{/snippet}
        </SchemaField>
      {/if}
    {/each}
  </div>

  {#if others.length}
    <Section title="Other parameters" count={others.length} collapsible open={false}>
      <div class="fields">
        {#each others as f (f.id)}
          {#if f.place === "body"}
            {@render body(f)}
          {:else}
            <SchemaField
              id={f.id}
              name={f.name}
              required={f.required}
              schema={f.schema}
              description={f.description}
              example={f.example}
              tag={PLACE_LABEL[f.place]}
              bind:value={form.values[f.id]}
              bind:touched={touched[f.id]}
            />
          {/if}
        {/each}
      </div>
    </Section>
  {/if}

  {#if error}<Banner tone="danger" data-testid="ready-error">{error}</Banner>{/if}

  <DialogFooter>
    {#snippet start()}
      <Checkbox bind:checked={dontAsk} label="Don't ask for this request" data-testid="ready-dont-ask" />
    {/snippet}
    <Button onclick={() => finish("send")} disabled={busy} data-testid="ready-send-anyway">Send anyway</Button>
    <Button variant="primary" icon="send" loading={busy} onclick={fill} data-testid="ready-fill">
      Fill and send <Kbd keys="Mod+Enter" size="sm" />
    </Button>
  </DialogFooter>
</div>

{#snippet layer(f: InputField)}
  {#if f.place === "variable"}
    {#if f.secret}
      <span class="muted small">Saved in the keychain for the environment</span>
    {:else}
      <SegmentedControl
        size="sm"
        label={`Where ${f.name} goes`}
        options={layerOptions}
        value={form.layers[f.id] ?? "environment"}
        onchange={(v) => (form.layers[f.id] = v as VarLayer)}
      />
    {/if}
  {/if}
{/snippet}

{#snippet body(f: InputField)}
  <div class="body" data-testid="ready-field" data-field={f.id}>
    <div class="baseurl-head">
      <span class="strong">Body</span>{#if f.required}<span class="req">*</span>{/if}
      {#if f.problem}<span class="muted small">{f.problem.message}</span>{/if}
      <span class="spacer"></span>
      {#if flat}
        <SegmentedControl
          size="sm"
          label="Edit the body as"
          options={[
            { value: "form", label: "Form" },
            { value: "json", label: "JSON" },
          ]}
          value={bodyMode}
          onchange={(v) => {
            if (v === "json" && flat) form.values[f.id] = bodyFromForm(flat, bodyValues);
            if (v === "form" && flat) bodyValues = formFromBody(form.values[f.id] ?? "", flat) ?? bodyValues;
            bodyMode = v as "form" | "json";
          }}
        />
      {/if}
    </div>
    {#if bodyMode === "form" && flat}
      <div class="body-form">
        {#each flat as p (p.name)}
          <SchemaField
            id={`body.${p.name}`}
            name={p.name}
            required={p.required}
            schema={p.schema}
            description={p.description}
            example={p.schema.example !== undefined ? String(p.schema.example) : null}
            bind:value={bodyValues[p.name]}
            bind:touched={touched[`body.${p.name}`]}
          />
        {/each}
      </div>
    {:else}
      <JsonSchemaEditor
        value={form.values[f.id] ?? ""}
        onchange={(v) => (form.values[f.id] = v)}
        schema={f.schema}
        label="Body (JSON)"
      />
      <div class="json-tools">
        <Button size="sm" variant="ghost" icon="generate" onclick={() => (form.values[f.id] = f.example ?? JSON.stringify(sample(f.schema), null, 2))}>Insert example</Button>
      </div>
    {/if}
  </div>
{/snippet}

<style>
  .ready {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-6);
  }
  .intro {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-1);
    margin: 0;
  }
  .muted {
    color: var(--rt-fg-muted);
  }
  .small {
    font: var(--rt-font-small);
  }
  .strong {
    font-weight: 600;
  }
  code.strong {
    font-family: var(--rt-font-mono);
    font-size: 12.5px;
  }
  .req {
    color: var(--rt-danger-fg);
  }
  .spacer {
    flex: 1;
  }
  .env-row {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--rt-space-4);
    padding: var(--rt-space-4) var(--rt-space-5);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-lg);
    background: var(--rt-surface-2);
  }
  .fields {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-6);
  }
  .baseurl,
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-3);
  }
  .baseurl-head {
    display: flex;
    align-items: center;
    gap: var(--rt-space-3);
    min-height: var(--rt-control-sm);
  }
  .body-form {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--rt-space-5) var(--rt-space-6);
    padding: var(--rt-space-5);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-lg);
    background: var(--rt-surface-2);
  }
  .json-tools {
    display: flex;
    margin-top: calc(-1 * var(--rt-space-2));
  }
  @media (max-width: 720px) {
    .body-form,
    .env-row {
      grid-template-columns: 1fr;
    }
  }
</style>
