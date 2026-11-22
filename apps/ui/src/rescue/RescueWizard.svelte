<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Scan the history for credentials and rewrite it, step by step, with the force-push warning
     and the hint to rotate (wave 12, strand G). Values never reach this component: findings
     carry masked snippets, the plan only finding ids. The history is rewritten only after the
     user ticked the confirmation; the backend makes the backup refs first and puts the value
     into the secret store; afterwards the collection is reloaded and the environment gets the
     secret variable (unsaved, like any other edit). No force push from here. -->
<script lang="ts">
  import type { HistoryFinding, HistoryScan, RescueResult, ScanProgress } from "../commands/types";
  import { getApp } from "../state/app.svelte";
  import {
    canRescue,
    deleteBackupCommands,
    forcePushCommands,
    shortOid,
    suggestVariable,
    validVariable,
    withSecretVariable,
  } from "./model";

  type Step = "start" | "scanning" | "results" | "plan" | "rewriting" | "done";

  const NEW_ENV = "\u0000new";
  const app = getApp();

  let step = $state<Step>("start");
  let progress = $state<ScanProgress | null>(null);
  let scan = $state<HistoryScan | null>(null);
  let chosen = $state<HistoryFinding | null>(null);
  let variable = $state("");
  let environment = $state("");
  let newEnvironment = $state("");
  let confirmed = $state(false);
  let error = $state<string | null>(null);
  let result = $state<RescueResult | null>(null);
  let declared = $state<string | null>(null);

  const envName = $derived(environment === NEW_ENV ? newEnvironment.trim() : environment);
  const unsaved = $derived(app.dirty.length > 0);
  const ready = $derived(confirmed && validVariable(variable) && envName.length > 0 && !unsaved);
  const pushes = $derived(result ? forcePushCommands(result) : []);

  function message(e: unknown): string {
    return e instanceof Error ? e.message : String(e);
  }

  /** "Add users (Ada, 1/2/2026) · in 3 commits · main, feature" */
  function where(f: HistoryFinding): string {
    const date = new Date(f.first.time * 1000).toLocaleDateString();
    const commits = `in ${f.commits.length} commit${f.commits.length === 1 ? "" : "s"}`;
    return [`${f.first.summary} (${f.first.author}, ${date})`, commits, ...(f.branches.length > 0 ? [f.branches.join(", ")] : [])].join(
      " · ",
    );
  }

  async function startScan() {
    step = "scanning";
    error = null;
    progress = null;
    scan = null;
    try {
      scan = await app.backend.rescue.scanHistory((p) => (progress = p));
      step = "results";
    } catch (e) {
      error = message(e);
      step = "start";
    }
  }

  function choose(f: HistoryFinding) {
    chosen = f;
    variable = suggestVariable(f);
    environment = app.environment ?? app.environments[0]?.name ?? NEW_ENV;
    newEnvironment = "";
    confirmed = false;
    error = null;
    step = "plan";
  }

  async function rewrite() {
    if (!chosen || !ready) return;
    const plan = { findings: [chosen.id], variable, environment: envName };
    step = "rewriting";
    error = null;
    try {
      result = await app.backend.rescue.rewrite(plan);
    } catch (e) {
      error = message(e);
      step = "plan";
      return;
    }
    step = "done";
    // The files changed underneath; then the environment needs the variable as a secret.
    try {
      await app.reload();
      const envs = await app.backend.environments();
      const updated = withSecretVariable(
        envs.find((e) => e.name === plan.environment),
        plan.environment,
        plan.variable,
      );
      if (updated) {
        await app.backend.updateEnvironment(updated);
        await app.reloadEnvironments();
        declared = plan.environment;
      }
    } catch (e) {
      error = message(e);
    }
  }
</script>

<section class="rescue" data-testid="rescue-wizard" aria-label="Secret rescue">
  {#if step === "start" || step === "scanning"}
    <p class="lead">
      Scans every commit of <code>HEAD</code> and your local branches for credentials committed in clear text: tokens,
      keys, passwords. A commit that removed a secret does not remove it from the history.
    </p>
    <p class="hint">The values stay on this device; you only see masked snippets.</p>
    <div class="actions">
      <button
        class="button primary"
        data-testid="rescue-scan"
        disabled={step === "scanning"}
        onclick={() => void startScan()}
      >
        {step === "scanning" ? "Scanning…" : "Scan history"}
      </button>
      {#if step === "scanning" && progress}
        <progress max={Math.max(progress.total, 1)} value={progress.scanned}></progress>
        <span class="muted" data-testid="rescue-progress">Commit {progress.scanned} of {progress.total}</span>
      {/if}
    </div>
  {:else if step === "results" && scan}
    {#if scan.findings.length === 0}
      <p class="ok" data-testid="rescue-clean">
        No credentials found in {scan.commits} commit{scan.commits === 1 ? "" : "s"}.
      </p>
    {:else}
      <p>
        Found {scan.findings.length} credential{scan.findings.length === 1 ? "" : "s"} in {scan.commits} commit{scan.commits ===
        1
          ? ""
          : "s"}:
      </p>
      <ul class="findings">
        {#each scan.findings as f (f.id)}
          <li class="finding" data-testid="rescue-finding">
            <div class="head">
              <span class="kind">{f.finding.kind}</span>
              <span class="path">{f.finding.path}:{f.finding.line}</span>
              {#if f.in_head}<span class="badge" title="The current files still contain it">in HEAD</span>{/if}
            </div>
            <code class="snippet">{f.finding.snippet}</code>
            <p class="hint">
              Since <span class="rev">{shortOid(f.first.oid)}</span>
              {where(f)}
            </p>
            {#if canRescue(f)}
              <button class="button" data-testid="rescue-choose" onclick={() => choose(f)}>Rescue…</button>
            {:else}
              <p class="hint">
                A private key cannot become a variable: remove the key file from the history with a tool such as
                <code>git filter-repo</code>, and replace the key.
              </p>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
    <div class="actions">
      <button class="button" data-testid="rescue-rescan" onclick={() => void startScan()}>Scan again</button>
    </div>
  {:else if (step === "plan" || step === "rewriting") && chosen}
    <h3>Replace the {chosen.finding.kind} in the history</h3>
    <code class="snippet">{chosen.finding.path}:{chosen.finding.line} · {chosen.finding.snippet}</code>
    <div class="fields">
      <label>
        Variable
        <input
          data-testid="rescue-variable"
          bind:value={variable}
          spellcheck="false"
          aria-invalid={!validVariable(variable)}
          disabled={step === "rewriting"}
        />
      </label>
      <label>
        Environment
        <select data-testid="rescue-environment" bind:value={environment} disabled={step === "rewriting"}>
          {#each app.environments as e (e.name)}
            <option value={e.name}>{e.name}</option>
          {/each}
          <option value={NEW_ENV}>New environment…</option>
        </select>
      </label>
      {#if environment === NEW_ENV}
        <label>
          Name
          <input data-testid="rescue-new-environment" bind:value={newEnvironment} disabled={step === "rewriting"} />
        </label>
      {/if}
    </div>
    {#if !validVariable(variable)}
      <p class="error">Use letters, digits, <code>_</code>, <code>-</code> and <code>.</code> for the variable.</p>
    {/if}
    <ul class="what" data-testid="rescue-explain">
      <li>
        Every file of every commit on
        <strong>{chosen.branches.length > 0 ? chosen.branches.join(", ") : "the local branches"}</strong>
        that holds the value gets <code>{`{{${variable || "…"}}}`}</code> instead. Other files, trees and commit messages
        stay as they are; the commit ids change.
      </li>
      <li>
        The value is kept as a secret of <strong>{envName || "…"}</strong> (OS keychain, or encrypted browser storage), never
        in a file.
      </li>
      <li>
        First each branch is saved under <code>refs/requesttailor/backup/&lt;time&gt;/&lt;branch&gt;</code>. These hidden
        refs still hold the secret: delete them once you are sure.
      </li>
      <li>Tags and remote branches are not changed. Anyone who cloned the repository keeps the old commits.</li>
    </ul>
    {#if unsaved}
      <p class="error" data-testid="rescue-unsaved">Save or discard your changes first.</p>
    {/if}
    <label class="confirm">
      <input type="checkbox" data-testid="rescue-confirm" bind:checked={confirmed} disabled={step === "rewriting"} />
      I understand that this rewrites the history of these branches.
    </label>
    <div class="actions">
      <button class="button danger" data-testid="rescue-rewrite" disabled={!ready || step === "rewriting"} onclick={() => void rewrite()}>
        {step === "rewriting" ? "Rewriting…" : "Rewrite history"}
      </button>
      <button class="button" data-testid="rescue-back" disabled={step === "rewriting"} onclick={() => (step = "results")}>
        Back
      </button>
    </div>
  {:else if step === "done" && result}
    <p class="ok" data-testid="rescue-done">
      Rewrote {result.rewritten} commit{result.rewritten === 1 ? "" : "s"} on {result.branches.map((b) => b.name).join(", ")}.
      The value is now <code>{`{{${variable}}}`}</code>, kept as a secret of <strong>{envName}</strong>.
    </p>
    {#if declared}
      <p class="hint" data-testid="rescue-declared">
        Added the secret variable <code>{variable}</code> to the environment {declared}: save and commit it.
      </p>
    {/if}
    {#if pushes.length > 0}
      <div class="warning" role="alert" data-testid="rescue-force-push">
        <strong>The secret is still on the remote.</strong>
        {pushes.length === 1 ? "This branch was" : "These branches were"} pushed before. Until you replace {pushes.length ===
        1
          ? "it"
          : "them"} there, everyone with access can read the secret in the old commits. RequestTailor does not push with force; check that nobody else pushed meanwhile, then run:
        {#each pushes as cmd (cmd)}
          <code class="command">{cmd}</code>
        {/each}
        Collaborators need to fetch and reset their branches afterwards.
      </div>
    {/if}
    <div class="warning" role="note" data-testid="rescue-rotate">
      <strong>Rotate the secret.</strong>
      It was exposed in the history{pushes.length > 0 ? " and on the remote" : ""}: revoke it at the provider, create a new
      one and set the new value in Environments → {envName} → {variable}.
    </div>
    {#if result.tags.length > 0}
      <p class="hint" data-testid="rescue-tags">
        Tags were not rewritten and still hold the secret: {result.tags.join(", ")}. Move or delete them by hand (also on the
        remote).
      </p>
    {/if}
    <details class="backups">
      <summary>Backups</summary>
      <p class="hint">
        The old branches are saved under <code>{result.backup}</code>. Delete them when you no longer need them:
      </p>
      {#each deleteBackupCommands(result) as cmd (cmd)}
        <code class="command">{cmd}</code>
      {/each}
    </details>
    <div class="actions">
      <button class="button" data-testid="rescue-rescan" onclick={() => void startScan()}>Scan again</button>
    </div>
  {/if}
  {#if error}
    <p class="error" data-testid="rescue-error">{error}</p>
  {/if}
</section>

<style>
  .rescue {
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
    padding: var(--rt-gap) 12px;
    max-width: 72em;
  }
  p,
  h3,
  ul {
    margin: 0;
  }
  h3 {
    font-size: var(--rt-size);
    font-weight: 600;
  }
  code {
    font-family: var(--rt-font-mono);
    font-size: var(--rt-size-small);
  }
  .actions,
  .fields,
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-gap) 12px;
  }
  .fields label {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  input:not([type="checkbox"]),
  select {
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    padding: 2px 6px;
  }
  input:not([type="checkbox"]) {
    font-family: var(--rt-font-mono);
    width: 14em;
  }
  input[aria-invalid="true"] {
    border-color: var(--rt-danger);
  }
  .button {
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg);
    color: var(--rt-fg);
    cursor: pointer;
    align-self: flex-start;
  }
  .button.primary:not(:disabled) {
    background: var(--rt-accent);
    border-color: var(--rt-accent);
    color: var(--rt-accent-fg);
  }
  .button.danger:not(:disabled) {
    background: var(--rt-danger);
    border-color: var(--rt-danger);
    color: var(--rt-accent-fg);
  }
  .button:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .hint,
  .muted {
    color: var(--rt-fg-muted);
  }
  .hint {
    font-size: var(--rt-size-small);
  }
  .error {
    color: var(--rt-danger);
  }
  .ok {
    color: var(--rt-ok);
  }
  .findings {
    list-style: none;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
  }
  .finding {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: var(--rt-gap);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg-panel);
  }
  .kind {
    font-weight: 600;
  }
  .path,
  .rev {
    font-family: var(--rt-font-mono);
    font-size: var(--rt-size-small);
  }
  .badge {
    font-size: var(--rt-size-small);
    padding: 0 6px;
    border-radius: var(--rt-radius);
    border: 1px solid var(--rt-warn);
    color: var(--rt-warn);
  }
  .snippet,
  .command {
    display: block;
    padding: 4px 6px;
    border-radius: var(--rt-radius);
    background: var(--rt-bg-panel);
    overflow-x: auto;
    white-space: pre;
  }
  .what {
    padding-left: 1.2em;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .confirm {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .warning {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: var(--rt-gap);
    border: 1px solid var(--rt-warn);
    border-left-width: 4px;
    border-radius: var(--rt-radius);
  }
  .warning .command {
    background: var(--rt-bg);
  }
  .backups {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .backups summary {
    cursor: pointer;
  }
</style>
