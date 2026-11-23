<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Manage environments and secret values. Owner: wave 3, strand D. Wave 5 mount points:
     ClientCertificates (strand D), ExternalSecrets (strand E); both edit the draft through
     `onchange`, which stores it like any other change.
     Every change goes to the backend at once (in memory) and is written to files by the global
     Save. Secret values never travel in an `Environment`: they go to `backend.secrets` (OS
     keychain on desktop, encrypted browser storage on the web).
     Wave 14: plain values are CodeFields with `{{` completion (collection variables and the
     selected environment's own); "Create variable" there adds a row to this environment. -->
<script lang="ts">
  import { untrack } from "svelte";

  import type { Environment, Variable } from "../commands/types";
  import CodeField from "../editor/CodeField.svelte";
  import { createVarSource, provideVarSource, withEnvironmentVariable } from "../editor/var-source";
  import { getApp } from "../state/app.svelte";
  import ClientCertificates from "./ClientCertificates.svelte";
  import ExternalSecrets from "./ExternalSecrets.svelte";
  import {
    createEnvironment,
    deleteEnvironment,
    duplicateEnvironment,
    isProductionName,
    makePlain,
    makeSecret,
    renameEnvironment,
    renameSecret,
    storeEnvironment,
    uniqueName,
    validate,
    withVariable,
  } from "./ops";

  let { open, onclose }: { open: boolean; onclose: () => void } = $props();
  const app = getApp();

  /** Name of the environment shown on the right. */
  let selected = $state<string | null>(null);
  /** Working copy of the selected environment (may hold rows without a name yet). */
  let draft = $state<Environment | null>(null);
  let creating = $state(false);
  let newName = $state("");
  let renameTo = $state("");
  let confirmDelete = $state(false);
  /** Row whose secret → plain switch waits for confirmation. */
  let confirmPlain = $state<number | null>(null);
  /** Revealed secret values by row. */
  let revealed = $state<Record<number, string>>({});
  /** Secret variables that have a stored value. */
  let stored = $state<Record<string, boolean>>({});
  let error = $state<string | null>(null);

  const names = $derived(app.environments.map((e) => e.name));
  /** Values are not highlighted by provenance here (no request to preview). */
  const NO_LOOKUP = new Map();
  const snapshot = (e: Environment): Environment => $state.snapshot(e) as Environment;

  // Operations run one after another, each on the then-current draft.
  let queue: Promise<void> = Promise.resolve();
  function run(op: () => Promise<void>): Promise<void> {
    queue = queue.then(async () => {
      try {
        error = null;
        await op();
      } catch (e) {
        error = e instanceof Error ? e.message : String(e);
      }
    });
    return queue;
  }

  /** Reloads the app's environments, marks files dirty and keeps the active one valid. */
  async function refresh() {
    await app.reloadEnvironments();
    app.dirty = await app.backend.dirty();
    if (app.environment !== null && !app.environments.some((e) => e.name === app.environment)) {
      app.setEnvironment(app.environments[0]?.name ?? null);
    }
  }

  async function select(name: string | null) {
    flushValue();
    const env = app.environments.find((e) => e.name === name) ?? app.environments[0] ?? null;
    selected = env?.name ?? null;
    draft = env ? snapshot(env) : null;
    renameTo = selected ?? "";
    confirmDelete = false;
    confirmPlain = null;
    revealed = {};
    stored = {};
    if (!draft) return;
    const env0 = draft.name;
    for (const v of draft.variables) {
      if (v.secret && v.name) stored[v.name] = await app.backend.secrets.has(env0, v.name);
    }
  }

  function opened(node: HTMLDialogElement) {
    node.showModal();
    untrack(() => {
      creating = false;
      error = null;
      void run(() => select(app.environment ?? app.environments[0]?.name ?? null));
    });
    return () => node.close();
  }

  /** Validates and stores `next` as the selected environment. */
  async function commit(next: Environment, before?: () => Promise<void>) {
    const problem = validate(next, names.filter((n) => n !== next.name));
    if (problem) throw new Error(problem);
    if (before) await before();
    await storeEnvironment(app.backend, next);
    draft = next;
    await refresh();
  }

  const current = () => {
    if (!draft) throw new Error("No environment selected.");
    return snapshot(draft);
  };

  function create() {
    const name = newName.trim();
    void run(async () => {
      const problem = validate({ name, variables: [], client_certificates: [], external_secrets: null }, names);
      if (problem) throw new Error(problem);
      await createEnvironment(app.backend, name);
      await refresh();
      creating = false;
      newName = "";
      await select(name);
    });
  }

  function rename() {
    const to = renameTo.trim();
    void run(async () => {
      const env = current();
      if (to === env.name) return;
      const problem = validate({ ...env, name: to }, names.filter((n) => n !== env.name));
      if (problem) throw new Error(problem);
      await renameEnvironment(app.backend, env, to);
      if (app.environment === env.name) app.setEnvironment(to);
      await refresh();
      await select(to);
    });
  }

  function duplicate() {
    void run(async () => {
      const env = current();
      const to = uniqueName(`${env.name} copy`, names);
      await duplicateEnvironment(app.backend, env, to);
      await refresh();
      await select(to);
    });
  }

  function remove() {
    void run(async () => {
      await deleteEnvironment(app.backend, current());
      await refresh();
      await select(app.environment ?? app.environments[0]?.name ?? null);
    });
  }

  // ---- plain values: typed into a CodeField, stored after a pause (wave 14) ----

  const VALUE_MS = 250;
  let valueTimer: ReturnType<typeof setTimeout> | undefined;

  /** Stores the draft now if a typed value is waiting. */
  function flushValue() {
    if (valueTimer === undefined) return;
    clearTimeout(valueTimer);
    valueTimer = undefined;
    if (!draft) return;
    const env = current();
    void run(async () => {
      const problem = validate(env, names.filter((n) => n !== env.name));
      if (problem) throw new Error(problem);
      // The draft stays as typed (typing may have gone on meanwhile).
      await storeEnvironment(app.backend, env);
      await refresh();
    });
  }

  function typeValue(i: number, value: string) {
    if (!draft || draft.variables[i]?.secret) return;
    draft.variables[i].value = value;
    clearTimeout(valueTimer);
    valueTimer = setTimeout(flushValue, VALUE_MS);
  }

  function close() {
    flushValue();
    onclose();
  }
  $effect(() => () => flushValue());

  // `{{` completion in values: the collection's and the selected environment's variables.
  provideVarSource(
    createVarSource(app, {
      path: () => "",
      environment: () => selected,
      addToEnvironment: async (_name, variable) => {
        flushValue();
        await queue;
        const next = withEnvironmentVariable(current(), variable);
        await commit(next);
        if (variable.secret) stored[variable.name] = true;
      },
    }),
  );

  function addVariable() {
    if (!draft) return;
    draft.variables.push({ name: "", value: "", enabled: true, secret: false });
  }

  function setName(i: number, name: string) {
    void run(async () => {
      const env = current();
      const v = env.variables[i];
      const next = withVariable(env, i, { ...v, name });
      await commit(next, async () => {
        if (v.secret) {
          await renameSecret(app.backend, env.name, v.name, name);
          stored[name] = stored[v.name] ?? false;
          delete stored[v.name];
        }
      });
    });
  }

  function setField(i: number, change: Partial<Variable>) {
    void run(async () => {
      const env = current();
      await commit(withVariable(env, i, { ...env.variables[i], ...change }));
    });
  }

  function setSecretValue(i: number, input: HTMLInputElement) {
    const value = input.value;
    void run(async () => {
      const env = current();
      const v = env.variables[i];
      if (!v.name) throw new Error("Name the variable before setting its value.");
      await app.backend.secrets.set(env.name, v.name, value);
      stored[v.name] = true;
      if (i in revealed) revealed[i] = value;
      else input.value = "";
    });
  }

  function toggleSecret(i: number, checked: boolean) {
    if (!checked) {
      // Secret → plain would write the value into a file: ask first.
      confirmPlain = i;
      return;
    }
    void run(async () => {
      const env = current();
      const v = env.variables[i];
      const next = await makeSecret(app.backend, env, i);
      draft = next;
      stored[v.name] = !!v.value;
      await refresh();
    });
  }

  function confirmMakePlain(i: number) {
    confirmPlain = null;
    void run(async () => {
      const next = await makePlain(app.backend, current(), i);
      draft = next;
      delete revealed[i];
      await refresh();
    });
  }

  function reveal(i: number) {
    if (i in revealed) {
      delete revealed[i];
      return;
    }
    void run(async () => {
      const env = current();
      revealed[i] = (await app.backend.secrets.get(env.name, env.variables[i].name)) ?? "";
    });
  }

  function removeVariable(i: number) {
    void run(async () => {
      const env = current();
      const v = env.variables[i];
      const next = { ...env, variables: env.variables.filter((_, k) => k !== i) };
      await commit(next, async () => {
        if (v.secret && v.name) await app.backend.secrets.delete(env.name, v.name);
      });
      revealed = {};
      confirmPlain = null;
    });
  }
</script>

{#if open}
  <dialog
    data-testid="env-manager"
    aria-label="Environments"
    {@attach opened}
    oncancel={(e) => {
      e.preventDefault();
      close();
    }}
  >
    <header>
      <h2>Environments</h2>
      <button data-testid="env-save" onclick={() => run(() => app.save())} title="Write all changes to the files">
        Save
      </button>
      <button data-testid="env-close" onclick={close}>Close</button>
    </header>

    {#if error}<p class="error" role="alert" data-testid="env-error">{error}</p>{/if}

    <div class="body">
      <aside aria-label="Environment list">
        <ul>
          {#each app.environments as env (env.name)}
            <li>
              <button
                data-testid="env-item"
                class:selected={env.name === selected}
                class:prod={isProductionName(env.name)}
                aria-current={env.name === selected ? "true" : undefined}
                onclick={() => run(() => select(env.name))}
              >
                {env.name}
                {#if env.name === app.environment}<span class="badge" title="Active">active</span>{/if}
              </button>
            </li>
          {/each}
        </ul>
        {#if creating}
          <form
            onsubmit={(e) => {
              e.preventDefault();
              create();
            }}
          >
            <!-- svelte-ignore a11y_autofocus -->
            <input data-testid="env-new-name" aria-label="New environment name" bind:value={newName} autofocus />
            <button data-testid="env-create" type="submit">Create</button>
            <button type="button" onclick={() => (creating = false)}>Cancel</button>
          </form>
        {:else}
          <button data-testid="env-new" onclick={() => (creating = true)}>New environment</button>
        {/if}
      </aside>

      <section aria-label="Environment details">
        {#if draft}
          <div class="toolbar">
            <form
              onsubmit={(e) => {
                e.preventDefault();
                rename();
              }}
            >
              <input data-testid="env-name" aria-label="Environment name" bind:value={renameTo} />
              <button data-testid="env-rename" type="submit" disabled={renameTo.trim() === draft.name}>
                Rename
              </button>
            </form>
            <button data-testid="env-duplicate" onclick={duplicate}>Duplicate</button>
            {#if confirmDelete}
              <span class="confirm">
                Delete “{draft.name}” and its secret values?
                <button data-testid="env-delete-confirm" class="danger" onclick={remove}>Delete</button>
                <button data-testid="env-delete-cancel" onclick={() => (confirmDelete = false)}>Cancel</button>
              </span>
            {:else}
              <button data-testid="env-delete" onclick={() => (confirmDelete = true)}>Delete</button>
            {/if}
          </div>

          <table>
            <thead>
              <tr>
                <th>Enabled</th>
                <th>Name</th>
                <th>Value</th>
                <th>Secret</th>
                <th><span class="visually-hidden">Actions</span></th>
              </tr>
            </thead>
            <tbody>
              {#each draft.variables as v, i (i)}
                <tr data-testid="env-var">
                  <td>
                    <input
                      data-testid="var-enabled"
                      type="checkbox"
                      aria-label="Enabled"
                      checked={v.enabled}
                      onchange={(e) => setField(i, { enabled: e.currentTarget.checked })}
                    />
                  </td>
                  <td>
                    <input
                      data-testid="var-name"
                      aria-label="Variable name"
                      value={v.name}
                      onchange={(e) => setName(i, e.currentTarget.value)}
                    />
                  </td>
                  <td class="value">
                    <div class="field">
                      {#if v.secret}
                        <input
                          data-testid="var-value"
                          aria-label="Secret value"
                          type={i in revealed ? "text" : "password"}
                          autocomplete="off"
                          disabled={!v.name}
                          placeholder={stored[v.name] ? "stored securely" : "secret value"}
                          value={revealed[i] ?? ""}
                          onchange={(e) => setSecretValue(i, e.currentTarget)}
                        />
                        <button
                          data-testid="var-reveal"
                          aria-pressed={i in revealed}
                          disabled={!v.name}
                          onclick={() => reveal(i)}>{i in revealed ? "Hide" : "Reveal"}</button
                        >
                      {:else}
                        <CodeField
                          testid="var-value"
                          label="Value"
                          value={v.value ?? ""}
                          lookup={NO_LOOKUP}
                          onchange={(value) => typeValue(i, value)}
                        />
                      {/if}
                    </div>
                    {#if confirmPlain === i}
                      <span class="confirm">
                        The value will be written to the environment file in plain text.
                        <button data-testid="var-plain-confirm" class="danger" onclick={() => confirmMakePlain(i)}>
                          Store in file
                        </button>
                        <button data-testid="var-plain-cancel" onclick={() => (confirmPlain = null)}>Keep secret</button>
                      </span>
                    {/if}
                  </td>
                  <td>
                    <input
                      data-testid="var-secret"
                      type="checkbox"
                      aria-label="Secret"
                      title={v.name ? "Keep the value out of files" : "Name the variable first"}
                      disabled={!v.name}
                      checked={v.secret && confirmPlain !== i}
                      onchange={(e) => toggleSecret(i, e.currentTarget.checked)}
                    />
                  </td>
                  <td>
                    <button data-testid="var-remove" aria-label="Remove variable" onclick={() => removeVariable(i)}>
                      ×
                    </button>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
          <button data-testid="var-add" onclick={addVariable}>Add variable</button>
          <ExternalSecrets env={draft} onchange={(next) => run(() => commit(next))} />
          <ClientCertificates env={draft} onchange={(next) => run(() => commit(next))} />
          <p class="hint">
            Secret values stay on this device ({app.backend.kind === "web"
              ? "encrypted in this browser"
              : app.backend.kind === "tauri"
                ? "in the system keychain"
                : "in memory"}); the files only name them.
          </p>
        {:else}
          <p class="hint">No environment yet. Create one to define variables.</p>
        {/if}
      </section>
    </div>
  </dialog>
{/if}

<style>
  dialog {
    width: min(860px, calc(100vw - 32px));
    max-height: calc(100vh - 64px);
    padding: 0;
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    font: inherit;
  }

  dialog::backdrop {
    background: rgb(0 0 0 / 35%);
  }

  header {
    display: flex;
    align-items: center;
    gap: var(--rt-gap);
    padding: var(--rt-gap) calc(var(--rt-gap) * 2);
    border-bottom: 1px solid var(--rt-border);
    background: var(--rt-bg-panel);
  }

  h2 {
    flex: 1;
    margin: 0;
    font-size: inherit;
  }

  .body {
    display: grid;
    grid-template-columns: 200px 1fr;
    min-height: 320px;
  }

  aside {
    padding: var(--rt-gap);
    border-right: 1px solid var(--rt-border);
    background: var(--rt-bg-panel);
  }

  ul {
    margin: 0 0 var(--rt-gap);
    padding: 0;
    list-style: none;
  }

  aside li button {
    display: flex;
    width: 100%;
    justify-content: space-between;
    border: 0;
    background: none;
    color: inherit;
    text-align: left;
    padding: 4px var(--rt-gap);
    border-radius: var(--rt-radius);
    font: inherit;
  }

  aside li button:hover {
    background: var(--rt-bg-hover);
  }

  aside li button.selected {
    background: var(--rt-bg-selected);
  }

  aside li button.prod {
    color: var(--rt-warn);
    font-weight: 600;
  }

  .badge {
    font-size: var(--rt-size-small);
    color: var(--rt-ok);
  }

  aside form input {
    width: 100%;
    box-sizing: border-box;
    margin-bottom: 4px;
  }

  section {
    padding: var(--rt-gap) calc(var(--rt-gap) * 2);
    overflow: auto;
  }

  .toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-gap);
    margin-bottom: var(--rt-gap);
  }

  table {
    width: 100%;
    border-collapse: collapse;
    margin-bottom: var(--rt-gap);
  }

  th {
    text-align: left;
    font-weight: normal;
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }

  td {
    padding: 2px 4px;
    vertical-align: top;
  }

  td.value {
    width: 55%;
  }

  .field {
    display: flex;
    gap: 4px;
  }

  .field input {
    flex: 1;
    min-width: 0;
  }

  input:not([type="checkbox"]) {
    font: inherit;
    font-family: var(--rt-font-mono);
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    padding: 2px 4px;
  }

  .confirm {
    display: block;
    margin-top: 4px;
    color: var(--rt-warn);
  }

  .danger {
    color: var(--rt-danger);
  }

  .error {
    margin: 0;
    padding: var(--rt-gap) calc(var(--rt-gap) * 2);
    color: var(--rt-danger);
  }

  .hint {
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }

  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
