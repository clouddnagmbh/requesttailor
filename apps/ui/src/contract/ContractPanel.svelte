<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The collection against the OpenAPI spec of its repository (wave 12, strand F): the spec
     found, the check report (problems per request, operations no request covers), the changes
     of the spec between two revisions with breaking ones marked, and the API changelog as
     Markdown (the text `rt changelog` writes). Talks only to `app.backend.contract`; there is
     no way to configure the spec here (it is `extensions.requesttailor.contract.spec` in
     opencollection.yml). -->
<script lang="ts">
  import { onMount } from "svelte";

    import type { ContractReport, ContractSpec, FindingKind, SpecChanges } from "../commands/types";
  import { getGitViews } from "../git/views.svelte";
  import { getApp } from "../state/app.svelte";

  const app = getApp();
  const views = getGitViews();
  const contract = app.backend.contract;

  const KINDS: Record<FindingKind, string> = {
    "no-operation": "No operation",
    "unknown-parameter": "Unknown parameter",
    "missing-parameter": "Missing parameter",
    "body-mismatch": "Body does not fit",
  };

  let spec = $state<ContractSpec | null>(null);
  let loaded = $state(false);
  let error = $state<string | null>(null);
  let rev = $state("");
  let report = $state<ContractReport | null>(null);
  let from = $state("HEAD~1");
  let to = $state("HEAD");
  let changes = $state<SpecChanges | null>(null);
  let changelog = $state<string | null>(null);
  let busy = $state<string | null>(null);
  let copied = $state(false);

  async function run<T>(what: string, f: () => Promise<T>): Promise<T | null> {
    busy = what;
    error = null;
    try {
      return await f();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      return null;
    } finally {
      busy = null;
    }
  }

  async function check(): Promise<void> {
    report = await run("check", () => contract.check(rev.trim() === "" ? null : rev.trim()));
  }

  async function showChanges(): Promise<void> {
    changelog = null;
    changes = await run("changes", () => contract.changes(from.trim(), to.trim()));
  }

  async function showChangelog(): Promise<void> {
    changelog = await run("changelog", () => contract.changelog(from.trim(), to.trim()));
  }

  async function copy(): Promise<void> {
    try {
      await navigator.clipboard.writeText(changelog ?? "");
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {
      // No clipboard (permissions, insecure context): the text is selectable.
    }
  }

  async function openRequest(path: string): Promise<void> {
    await app.select(path);
    views.close();
  }

  onMount(async () => {
    spec = await run("spec", () => contract.spec());
    loaded = true;
    if (spec) await check();
  });

  const breakingCount = $derived(changes ? changes.changes.filter((c) => c.breaking).length : 0);
</script>

<div class="contract" data-testid="contract-panel">
  {#if !loaded}
    <p class="muted">Looking for the OpenAPI spec…</p>
  {:else if !spec}
    <div class="card" data-testid="contract-none">
      <strong>No OpenAPI spec found</strong>
      <p>
        Put <code>openapi.yaml</code> (or <code>.yml</code>, <code>.json</code>) next to the collection or at the
        repository root, or name the file in <code>opencollection.yml</code> under
        <code>extensions.requesttailor.contract.spec</code>.
      </p>
      {#if error}<p class="error" role="alert">{error}</p>{/if}
    </div>
  {:else}
    <section class="card" aria-label="Spec">
      <div class="head">
        <strong data-testid="contract-spec-title">{spec.title}</strong>
        <span class="muted" data-testid="contract-spec-version">{spec.version}</span>
        <code class="muted" data-testid="contract-spec-path">{spec.path}</code>
        <span class="muted">{spec.configured ? "named in opencollection.yml" : "found next to the collection"}</span>
      </div>
    </section>

    {#if error}<p class="error" role="alert" data-testid="contract-error">{error}</p>{/if}

    <section class="card" aria-label="Check">
      <div class="row">
        <label>
          Revision
          <input data-testid="contract-rev" bind:value={rev} placeholder="saved files" />
        </label>
        <button data-testid="contract-check" disabled={busy !== null} onclick={check}>Check</button>
      </div>
      {#if report}
        <p data-testid="contract-summary">
          {report.covered} of {report.operations} operations are covered by a request,
          {report.findings.length} {report.findings.length === 1 ? "problem" : "problems"} found.
        </p>
        {#if report.findings.length > 0}
          <ul class="list" data-testid="contract-findings">
            {#each report.findings as f, i (i)}
              <li data-testid="contract-finding" data-kind={f.kind}>
                <span class="kind">{KINDS[f.kind]}</span>
                <button class="link" onclick={() => openRequest(f.request)}>{f.request}</button>
                <span class="message">{f.message}</span>
              </li>
            {/each}
          </ul>
        {/if}
        {#if report.uncovered.length > 0}
          <h3>No request for</h3>
          <ul class="list" data-testid="contract-uncovered">
            {#each report.uncovered as op (op)}
              <li data-testid="contract-uncovered-op"><code>{op}</code></li>
            {/each}
          </ul>
        {/if}
      {/if}
    </section>

    <section class="card" aria-label="Changes">
      <div class="row">
        <label>
          From
          <input data-testid="contract-from" bind:value={from} />
        </label>
        <label>
          To
          <input data-testid="contract-to" bind:value={to} />
        </label>
        <button data-testid="contract-changes" disabled={busy !== null} onclick={showChanges}>Show changes</button>
        <button data-testid="contract-changelog" disabled={busy !== null} onclick={showChangelog}>Changelog</button>
      </div>
      {#if changes}
        <p data-testid="contract-changes-summary">
          {changes.from_version ?? "no spec"} → {changes.to_version ?? "no spec"}:
          <strong class:danger={breakingCount > 0}>{breakingCount} breaking</strong>,
          {changes.changes.length - breakingCount} other.
        </p>
        <ul class="list" data-testid="contract-change-list">
          {#each changes.changes as c, i (i)}
            <li data-testid="contract-change" data-breaking={c.breaking}>
              <span class="badge" class:breaking={c.breaking}>{c.breaking ? "Breaking" : "Change"}</span>
              {#if c.operation}<code>{c.operation}</code>{/if}
              <span class="message">{c.message}</span>
              {#each c.requests as r (r)}
                <button class="link" onclick={() => openRequest(r)}>{r}</button>
              {/each}
            </li>
          {/each}
        </ul>
      {/if}
      {#if changelog !== null}
        <div class="row">
          <button data-testid="contract-copy" onclick={copy}>{copied ? "Copied" : "Copy Markdown"}</button>
        </div>
        <pre data-testid="contract-changelog-text">{changelog}</pre>
      {/if}
    </section>
  {/if}
</div>

<style>
  .contract {
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
    padding: var(--rt-gap);
    box-sizing: border-box;
    font-size: var(--rt-size);
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: var(--rt-gap);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg-panel);
  }
  p,
  h3 {
    margin: 0;
  }
  h3 {
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
    font-weight: 600;
  }
  .head,
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--rt-gap);
  }
  .head {
    align-items: baseline;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
  }
  input {
    min-width: 0;
    box-sizing: border-box;
    padding: 3px 6px;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg);
    color: var(--rt-fg);
    font-size: var(--rt-size);
  }
  button {
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg);
    color: var(--rt-fg);
    cursor: pointer;
    white-space: nowrap;
  }
  button:hover:not(:disabled) {
    background: var(--rt-bg-hover);
  }
  button:disabled {
    opacity: 0.6;
    cursor: default;
  }
  button.link {
    height: auto;
    padding: 0;
    border: none;
    background: none;
    color: var(--rt-accent);
    font-family: var(--rt-font-mono);
    font-size: var(--rt-size-small);
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .list li {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 6px;
  }
  code {
    font-family: var(--rt-font-mono);
    font-size: var(--rt-size-small);
  }
  .kind,
  .badge {
    padding: 0 6px;
    border-radius: var(--rt-radius);
    border: 1px solid var(--rt-border);
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
  }
  .badge.breaking {
    color: var(--rt-danger);
    border-color: var(--rt-danger);
  }
  .danger {
    color: var(--rt-danger);
  }
  .message {
    min-width: 0;
  }
  .muted {
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
  .error {
    color: var(--rt-danger);
  }
  pre {
    margin: 0;
    padding: var(--rt-gap);
    overflow: auto;
    max-height: 320px;
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    font-family: var(--rt-font-mono);
    font-size: var(--rt-size-small);
    user-select: text;
  }
</style>
