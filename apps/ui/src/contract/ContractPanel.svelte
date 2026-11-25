<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The collection against the OpenAPI spec of its repository (wave 12, strand F): the spec
     found, the check report (problems per request, operations no request covers), the changes
     of the spec between two revisions with breaking ones marked, and the API changelog as
     Markdown (the text `rt changelog` writes). Since wave 18 the same range also lists the
     changes of a GraphQL schema and of Protobuf files (strand H). Talks only to `app.backend.contract`; there is
     no way to configure the spec here (it is `extensions.requesttailor.contract.spec` in
     opencollection.yml). -->
<script lang="ts">
  import { onMount } from "svelte";

  import type { ContractReport, ContractSpec, FindingKind, SchemaKind, SpecChange, SpecChanges } from "../commands/types";
  import { getGitViews } from "../git/views.svelte";
  import { getApp } from "../state/app.svelte";
  import { Badge, Button, Card, EmptyState, Icon, Spinner, TextField } from "../ui";

  const app = getApp();
  const views = getGitViews();
  const contract = app.backend.contract;

  const KINDS: Record<FindingKind, string> = {
    "no-operation": "No operation",
    "unknown-parameter": "Unknown parameter",
    "missing-parameter": "Missing parameter",
    "body-mismatch": "Body does not fit",
  };

  const KIND_TONE: Record<FindingKind, "danger" | "warn"> = {
    "no-operation": "danger",
    "unknown-parameter": "warn",
    "missing-parameter": "warn",
    "body-mismatch": "danger",
  };

  let spec = $state<ContractSpec | null>(null);
  let loaded = $state(false);
  let error = $state<string | null>(null);
  let rev = $state("");
  let report = $state<ContractReport | null>(null);
  let from = $state("HEAD~1");
  let to = $state("HEAD");
  let changes = $state<SpecChanges | null>(null);
  /** The GraphQL and Protobuf changes of the range; an error is shown, not hidden. */
  interface SchemaResult {
    kind: SchemaKind;
    title: string;
    changes: SpecChange[];
    error: string | null;
  }
  let schemas = $state<SchemaResult[]>([]);
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
    schemas = [];
    const [a, b] = [from.trim(), to.trim()];
    changes = await run("changes", () => contract.changes(a, b));
    if (!changes) return;
    const kinds: [SchemaKind, string][] = [
      ["graphql", "GraphQL"],
      ["protobuf", "Protobuf"],
    ];
    schemas = await Promise.all(
      kinds.map(async ([kind, title]): Promise<SchemaResult> => {
        try {
          return { kind, title, changes: (await contract.schemaChanges(kind, a, b)).changes, error: null };
        } catch (e) {
          return { kind, title, changes: [], error: e instanceof Error ? e.message : String(e) };
        }
      }),
    );
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
    <p class="loading"><Spinner size="sm" label="Looking for the OpenAPI spec" /> Looking for the OpenAPI spec…</p>
  {:else if !spec}
    <div data-testid="contract-none">
      <EmptyState icon="contract" title="No OpenAPI spec found">
        <p class="none-text">
          Put <code>openapi.yaml</code> (or <code>.yml</code>, <code>.json</code>) next to the collection or at the
          repository root, or name the file in <code>opencollection.yml</code> under
          <code>extensions.requesttailor.contract.spec</code>.
        </p>
        {#if error}<p class="error" role="alert">{error}</p>{/if}
      </EmptyState>
    </div>
  {:else}
    <header class="page-head" aria-label="Spec">
      <span class="icon"><Icon name="contract" /></span>
      <div class="titles">
        <div class="line">
          <strong class="spec-title" data-testid="contract-spec-title">{spec.title}</strong>
          <span class="version" data-testid="contract-spec-version">{spec.version}</span>
        </div>
        <div class="line sub">
          <code data-testid="contract-spec-path">{spec.path}</code>
          <span>{spec.configured ? "named in opencollection.yml" : "found next to the collection"}</span>
        </div>
      </div>
    </header>

    {#if error}<p class="error" role="alert" data-testid="contract-error">{error}</p>{/if}

    <Card title="Check" description="Compare the requests with the operations of the spec.">
      <div class="stack">
      <div class="row">
        <div class="grow"><TextField label="Revision" data-testid="contract-rev" bind:value={rev} placeholder="saved files" mono /></div>
        <Button variant="primary" icon="check" data-testid="contract-check" disabled={busy !== null} loading={busy === "check"} onclick={check}>Check</Button>
      </div>
      {#if report}
        <div class="tiles">
          <div class="tile"><span class="num">{report.covered}<span class="of"> / {report.operations}</span></span><span class="cap">operations covered</span></div>
          <div class="tile" class:bad={report.findings.length > 0}><span class="num">{report.findings.length}</span><span class="cap">{report.findings.length === 1 ? "problem" : "problems"}</span></div>
          <div class="tile" class:warn={report.uncovered.length > 0}><span class="num">{report.uncovered.length}</span><span class="cap">without a request</span></div>
        </div>
        <p class="summary" data-testid="contract-summary">
          {report.covered} of {report.operations} operations are covered by a request,
          {report.findings.length} {report.findings.length === 1 ? "problem" : "problems"} found.
        </p>
        {#if report.findings.length > 0}
          <ul class="list" data-testid="contract-findings">
            {#each report.findings as f, i (i)}
              <li data-testid="contract-finding" data-kind={f.kind}>
                <Badge value={KINDS[f.kind]} tone={KIND_TONE[f.kind]} />
                <Button variant="ghost" size="sm" class="path-link" onclick={() => openRequest(f.request)}>{f.request}</Button>
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
      </div>
    </Card>

    <Card title="Changes" description="What changed in the spec between two revisions.">
      <div class="stack">
      <div class="row">
        <div class="grow"><TextField label="From" data-testid="contract-from" bind:value={from} mono /></div>
        <div class="grow"><TextField label="To" data-testid="contract-to" bind:value={to} mono /></div>
        <Button variant="primary" data-testid="contract-changes" disabled={busy !== null} loading={busy === "changes"} onclick={showChanges}>Show changes</Button>
        <Button icon="file-text" data-testid="contract-changelog" disabled={busy !== null} loading={busy === "changelog"} onclick={showChangelog}>Changelog</Button>
      </div>
      {#if changes}
        <p class="summary" data-testid="contract-changes-summary">
          {changes.from_version ?? "no spec"} → {changes.to_version ?? "no spec"}:
          <strong class:danger={breakingCount > 0}>{breakingCount} breaking</strong>,
          {changes.changes.length - breakingCount} other.
        </p>
        <ul class="list" data-testid="contract-change-list">
          {#each changes.changes as c, i (i)}
            <li data-testid="contract-change" data-breaking={c.breaking}>
              <Badge value={c.breaking ? "Breaking" : "Change"} tone={c.breaking ? "danger" : "neutral"} />
              {#if c.operation}<code>{c.operation}</code>{/if}
              <span class="message">{c.message}</span>
              {#each c.requests as r (r)}
                <Button variant="ghost" size="sm" class="path-link" onclick={() => openRequest(r)}>{r}</Button>
              {/each}
            </li>
          {/each}
        </ul>
      {/if}
      {#each schemas.filter((x) => x.changes.length > 0 || x.error) as schema (schema.kind)}
        <h3 data-testid="contract-schema-title">{schema.title}</h3>
        {#if schema.error}
          <p class="error" role="alert" data-testid="contract-schema-error">{schema.error}</p>
        {:else}
          <p class="summary" data-testid="contract-schema-summary" data-schema={schema.kind}>
            <strong class:danger={schema.changes.some((c) => c.breaking)}>{schema.changes.filter((c) => c.breaking).length} breaking</strong>,
            {schema.changes.filter((c) => !c.breaking).length} other.
          </p>
          <ul class="list" data-testid="contract-schema-list">
            {#each schema.changes as c, i (i)}
              <li data-testid="contract-schema-change" data-schema={schema.kind} data-breaking={c.breaking}>
                <Badge value={c.breaking ? "Breaking" : "Change"} tone={c.breaking ? "danger" : "neutral"} />
                {#if c.operation}<code>{c.operation}</code>{/if}
                <span class="message">{c.message}</span>
                {#each c.requests as r (r)}
                  <Button variant="ghost" size="sm" class="path-link" onclick={() => openRequest(r)}>{r}</Button>
                {/each}
              </li>
            {/each}
          </ul>
        {/if}
      {/each}
      {#if changelog !== null}
        <div class="changelog-head">
          <h3>Changelog (Markdown)</h3>
          <Button size="sm" icon="copy" data-testid="contract-copy" onclick={copy}>{copied ? "Copied" : "Copy Markdown"}</Button>
        </div>
        <pre data-testid="contract-changelog-text">{changelog}</pre>
      {/if}
      </div>
    </Card>
  {/if}
</div>

<style>
  .contract {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-6);
    padding: var(--rt-space-6);
    box-sizing: border-box;
    font: var(--rt-font-body);
    color: var(--rt-fg);
  }
  p,
  h3 {
    margin: 0;
  }
  h3 {
    font: var(--rt-font-caption);
    color: var(--rt-fg-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .loading {
    display: flex;
    align-items: center;
    gap: var(--rt-space-4);
    color: var(--rt-fg-muted);
  }
  .none-text {
    max-width: 56ch;
    font: var(--rt-font-small);
    color: var(--rt-fg-muted);
  }
  .page-head {
    display: flex;
    align-items: center;
    gap: var(--rt-space-5);
  }
  .icon {
    display: inline-grid;
    place-items: center;
    width: var(--rt-control-lg);
    height: var(--rt-control-lg);
    border-radius: var(--rt-radius-md);
    background: var(--rt-accent-subtle);
    color: var(--rt-accent-subtle-fg);
    flex: none;
  }
  .titles {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-1);
    min-width: 0;
  }
  .line {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--rt-space-2) var(--rt-space-4);
  }
  .spec-title {
    font: var(--rt-font-heading);
  }
  .version {
    padding: 0 var(--rt-space-3);
    border-radius: var(--rt-radius-sm);
    background: var(--rt-surface-sunken);
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
    font-family: var(--rt-font-mono);
  }
  .sub {
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .stack {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-6);
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--rt-space-5);
  }
  .grow {
    flex: 1 1 160px;
    min-width: 0;
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
    gap: var(--rt-space-5);
  }
  .tile {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-1);
    padding: var(--rt-space-5) var(--rt-space-6);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-md);
    background: var(--rt-surface-sunken);
  }
  .tile.bad {
    background: var(--rt-danger-bg);
    border-color: var(--rt-danger-border);
    color: var(--rt-danger-fg);
  }
  .tile.warn {
    background: var(--rt-warn-bg);
    border-color: var(--rt-warn-border);
    color: var(--rt-warn-fg);
  }
  .num {
    font: var(--rt-font-heading);
    font-variant-numeric: tabular-nums;
  }
  .of {
    color: var(--rt-fg-muted);
    font: var(--rt-font-body);
  }
  .cap {
    font: var(--rt-font-small);
    color: var(--rt-fg-muted);
  }
  .tile.bad .cap,
  .tile.warn .cap {
    color: inherit;
  }
  .summary {
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .summary .danger {
    color: var(--rt-danger-fg);
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
  }
  .list li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-2) var(--rt-space-4);
    padding: var(--rt-space-4) 0;
    border-top: 1px solid var(--rt-border);
  }
  .list li:first-child {
    border-top: none;
  }
  :global(.path-link) {
    font-family: var(--rt-font-mono);
  }
  code {
    font-family: var(--rt-font-mono);
    font-size: var(--rt-text-small);
  }
  .message {
    min-width: 0;
    overflow-wrap: anywhere;
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .changelog-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--rt-space-5);
  }
  .error {
    color: var(--rt-danger-fg);
  }
  pre {
    margin: 0;
    padding: var(--rt-space-5);
    overflow: auto;
    max-height: 320px;
    background: var(--rt-surface-sunken);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-md);
    font-family: var(--rt-font-mono);
    font-size: var(--rt-text-small);
    line-height: var(--rt-leading-small);
    user-select: text;
  }
</style>
