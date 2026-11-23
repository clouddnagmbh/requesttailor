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
  import { Badge, Banner, Button, Card, Checkbox, EmptyState, Progress, Select, TextField } from "../ui";
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
  <header>
    <p class="muted">Find credentials in the Git history and replace them with a secret variable.</p>
  </header>

  {#if step === "start" || step === "scanning"}
    <Card icon="shield-secret" title="Scan the history">
      <div class="stack">
        <p>
          Scans every commit of <code>HEAD</code> and your local branches for credentials committed in clear text: tokens,
          keys, passwords. A commit that removed a secret does not remove it from the history.
        </p>
        <p class="hint">The values stay on this device; you only see masked snippets.</p>
        <div class="actions">
          <Button variant="primary" data-testid="rescue-scan" loading={step === "scanning"} disabled={step === "scanning"} onclick={() => void startScan()}>
            {step === "scanning" ? "Scanning…" : "Scan history"}
          </Button>
          {#if step === "scanning" && progress}
            <span class="scan-progress"><Progress max={Math.max(progress.total, 1)} value={progress.scanned} label="Scanning the history" /></span>
            <span class="muted" data-testid="rescue-progress">Commit {progress.scanned} of {progress.total}</span>
          {/if}
        </div>
      </div>
    </Card>
  {:else if step === "results" && scan}
    {#if scan.findings.length === 0}
      <div data-testid="rescue-clean">
        <EmptyState icon="success" title="No credentials found in {scan.commits} commit{scan.commits === 1 ? '' : 's'}.">
          {#snippet actions()}
            <Button data-testid="rescue-rescan" onclick={() => void startScan()}>Scan again</Button>
          {/snippet}
        </EmptyState>
      </div>
    {:else}
      <div class="head">
        <h3>
          Found {scan.findings.length} credential{scan.findings.length === 1 ? "" : "s"} in {scan.commits} commit{scan.commits ===
          1
            ? ""
            : "s"}
        </h3>
        <span class="spacer"></span>
        <Button variant="ghost" data-testid="rescue-rescan" icon="sync" onclick={() => void startScan()}>Scan again</Button>
      </div>
      <ul class="findings">
        {#each scan.findings as f (f.id)}
          <li data-testid="rescue-finding">
            <Card padding="md">
              <div class="stack">
                <div class="head">
                  <strong class="kind">{f.finding.kind}</strong>
                  <span class="path">{f.finding.path}:{f.finding.line}</span>
                  {#if f.in_head}<Badge tone="warn" value="in HEAD" title="The current files still contain it" />{/if}
                </div>
                <code class="snippet">{f.finding.snippet}</code>
                <p class="hint">
                  Since <span class="rev">{shortOid(f.first.oid)}</span>
                  {where(f)}
                </p>
                {#if canRescue(f)}
                  <div class="actions">
                    <Button variant="primary" data-testid="rescue-choose" onclick={() => choose(f)}>Rescue…</Button>
                  </div>
                {:else}
                  <p class="hint">
                    A private key cannot become a variable: remove the key file from the history with a tool such as
                    <code>git filter-repo</code>, and replace the key.
                  </p>
                {/if}
              </div>
            </Card>
          </li>
        {/each}
      </ul>
    {/if}
  {:else if (step === "plan" || step === "rewriting") && chosen}
    <Card title="Replace the {chosen.finding.kind} in the history">
      <div class="stack">
        <code class="snippet">{chosen.finding.path}:{chosen.finding.line} · {chosen.finding.snippet}</code>
        <div class="fields">
          <TextField
            label="Variable"
            mono
            data-testid="rescue-variable"
            bind:value={variable}
            error={validVariable(variable) ? undefined : "Use letters, digits, _, - and . for the variable."}
            disabled={step === "rewriting"}
          />
          <Select
            label="Environment"
            data-testid="rescue-environment"
            bind:value={environment}
            disabled={step === "rewriting"}
            options={[...app.environments.map((e) => ({ value: e.name, label: e.name })), { value: NEW_ENV, label: "New environment…" }]}
          />
          {#if environment === NEW_ENV}
            <TextField label="Name" data-testid="rescue-new-environment" bind:value={newEnvironment} disabled={step === "rewriting"} />
          {/if}
        </div>
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
          <div data-testid="rescue-unsaved"><Banner tone="warn">Save or discard your changes first.</Banner></div>
        {/if}
        <Checkbox
          data-testid="rescue-confirm"
          bind:checked={confirmed}
          disabled={step === "rewriting"}
          label="I understand that this rewrites the history of these branches."
        />
        <div class="actions">
          <Button variant="danger" data-testid="rescue-rewrite" loading={step === "rewriting"} disabled={!ready || step === "rewriting"} onclick={() => void rewrite()}>
            {step === "rewriting" ? "Rewriting…" : "Rewrite history"}
          </Button>
          <Button variant="ghost" data-testid="rescue-back" disabled={step === "rewriting"} onclick={() => (step = "results")}>
            Back
          </Button>
        </div>
      </div>
    </Card>
  {:else if step === "done" && result}
    <div data-testid="rescue-done">
      <Banner tone="ok">
        Rewrote {result.rewritten} commit{result.rewritten === 1 ? "" : "s"} on {result.branches.map((b) => b.name).join(", ")}.
        The value is now <code>{`{{${variable}}}`}</code>, kept as a secret of <strong>{envName}</strong>.
      </Banner>
    </div>
    {#if declared}
      <p class="hint" data-testid="rescue-declared">
        Added the secret variable <code>{variable}</code> to the environment {declared}: save and commit it.
      </p>
    {/if}
    {#if pushes.length > 0}
      <div data-testid="rescue-force-push">
        <Banner tone="warn" title="The secret is still on the remote.">
          <div class="stack">
            <span>
              {pushes.length === 1 ? "This branch was" : "These branches were"} pushed before. Until you replace {pushes.length ===
              1
                ? "it"
                : "them"} there, everyone with access can read the secret in the old commits. RequestTailor does not push with force; check that nobody else pushed meanwhile, then run:
            </span>
            {#each pushes as cmd (cmd)}
              <code class="command">{cmd}</code>
            {/each}
            <span>Collaborators need to fetch and reset their branches afterwards.</span>
          </div>
        </Banner>
      </div>
    {/if}
    <div data-testid="rescue-rotate">
      <Banner tone="warn" title="Rotate the secret.">
        It was exposed in the history{pushes.length > 0 ? " and on the remote" : ""}: revoke it at the provider, create a new
        one and set the new value in Environments → {envName} → {variable}.
      </Banner>
    </div>
    {#if result.tags.length > 0}
      <p class="hint" data-testid="rescue-tags">
        Tags were not rewritten and still hold the secret: {result.tags.join(", ")}. Move or delete them by hand (also on the
        remote).
      </p>
    {/if}
    <Card padding="sm">
      <details class="backups">
        <summary>Backups</summary>
        <div class="stack">
          <p class="hint">
            The old branches are saved under <code>{result.backup}</code>. Delete them when you no longer need them:
          </p>
          {#each deleteBackupCommands(result) as cmd (cmd)}
            <code class="command">{cmd}</code>
          {/each}
        </div>
      </details>
    </Card>
    <div class="actions">
      <Button data-testid="rescue-rescan" onclick={() => void startScan()}>Scan again</Button>
    </div>
  {/if}
  {#if error}
    <div data-testid="rescue-error"><Banner tone="danger">{error}</Banner></div>
  {/if}
</section>

<style>
  .rescue {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-6);
    padding: var(--rt-space-6);
    max-width: 72em;
  }
  p,
  h3,
  ul {
    margin: 0;
  }
  h3 {
    font: var(--rt-font-title);
  }
  code {
    font-family: var(--rt-font-mono);
    font-size: var(--rt-text-small);
  }
  .stack {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-4);
  }
  .actions,
  .fields,
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-4) var(--rt-space-5);
  }
  .fields {
    align-items: flex-start;
  }
  .spacer {
    flex: 1;
  }
  .hint,
  .muted {
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .findings {
    list-style: none;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-5);
  }
  .kind {
    font: var(--rt-font-title);
  }
  .path,
  .rev {
    font-family: var(--rt-font-mono);
    font-size: var(--rt-text-small);
    overflow-wrap: anywhere;
  }
  .snippet,
  .command {
    display: block;
    padding: var(--rt-space-3) var(--rt-space-4);
    border-radius: var(--rt-radius-md);
    background: var(--rt-surface-sunken);
    border: 1px solid var(--rt-border);
    overflow-x: auto;
    white-space: pre;
  }
  .what {
    padding-left: var(--rt-space-7);
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-3);
  }
  .backups summary {
    cursor: pointer;
  }
  .scan-progress {
    flex: 0 1 16rem;
    min-width: 8rem;
  }
</style>
