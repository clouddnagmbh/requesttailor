<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Consumer contracts without a broker (wave 19, strand F): the collections your repository lists
     as consumers (extensions.requesttailor.consumers in opencollection.yml) run against the
     provider you built, at the base URL you give. A consumer that breaks is named with the request
     and the assertion; the answer itself is never shown. Nothing is sent until "Verify". -->
<script lang="ts">
  import { onMount } from "svelte";

  import type { ConsumerRef, ConsumerReport } from "../commands/types";
  import type { ToolProps } from "../shell/tool-host.svelte";
  import { Badge, Banner, Button, Checkbox, EmptyState, Spinner, TextField } from "../ui";
  import { WRITES_HINT, details, headline, isBroken, sourceLine, summary, tone } from "./model";

  let { app }: ToolProps = $props();

  let consumers = $state<ConsumerRef[] | null>(null);
  let baseUrl = $state("http://localhost:8080");
  let fetchAgain = $state(false);
  let trustScripts = $state(false);
  let allowWrites = $state(false);
  let selected = $state<Record<string, boolean>>({});

  let running = $state(false);
  let error = $state<string | null>(null);
  let report = $state<ConsumerReport | null>(null);
  let seq = 0;

  const message = (e: unknown) => (e instanceof Error ? e.message : String(e));
  const chosen = $derived((consumers ?? []).filter((c) => selected[c.name]));
  const ready = $derived(baseUrl.trim() !== "" && chosen.length > 0);

  onMount(async () => {
    try {
      const list = await app.backend.consumers.list();
      selected = Object.fromEntries(list.map((c) => [c.name, true]));
      consumers = list;
    } catch (e) {
      error = message(e);
      consumers = [];
    }
  });

  async function verify() {
    if (running || !ready) return;
    const mine = ++seq;
    running = true;
    error = null;
    report = null;
    try {
      const all = chosen.length === (consumers ?? []).length;
      const result = await app.backend.consumers.verify({
        base_url: baseUrl.trim(),
        only: all ? [] : chosen.map((c) => c.name),
        fetch: fetchAgain,
        keep_going: true,
        trust_scripts: trustScripts,
        allow_writes: allowWrites,
      });
      if (mine === seq) report = result;
    } catch (e) {
      if (mine === seq) error = message(e);
    } finally {
      if (mine === seq) running = false;
    }
  }
</script>

<section class="consumers" data-testid="consumers-panel" aria-label="Verify consumers">
  <header>
    <h2>Verify consumers</h2>
    <p class="note">
      Runs the collections your repository lists as consumers against the provider you built. Nothing is sent until you press Verify, and nothing is uploaded: a clone, if any, comes from your own Git host.
    </p>
  </header>

  {#if consumers === null}
    <div class="loading"><Spinner label="Reading" /> Reading the list of consumers.</div>
  {:else if consumers.length === 0}
    <EmptyState
      icon="collection"
      title="No consumers listed"
      text="List them in opencollection.yml under extensions.requesttailor.consumers: a name, a source (a folder or a Git URL), an environment."
    />
  {:else}
    <ul class="list" data-testid="consumers-list">
      {#each consumers as c (c.name)}
        <li class="row" data-testid="consumer-row" data-name={c.name}>
          <Checkbox label={c.name} bind:checked={selected[c.name]} disabled={running} />
          <span class="mono source">{sourceLine(c.source, c.ref)}</span>
          <Badge value={c.environment} />
        </li>
      {/each}
    </ul>

    <form
      class="form"
      onsubmit={(e) => {
        e.preventDefault();
        void verify();
      }}
    >
      <div class="field">
        <TextField label="Provider base URL" bind:value={baseUrl} mono disabled={running} data-testid="consumers-base-url" />
      </div>
      <div class="go">
        <Button type="submit" variant="primary" icon="check" loading={running} disabled={!ready} data-testid="consumers-verify">
          {running ? "Verifying…" : "Verify"}
        </Button>
      </div>
      <div class="checks">
        <Checkbox label="Fetch remote consumers again" bind:checked={fetchAgain} disabled={running} data-testid="consumers-fetch" />
        <Checkbox label="Let consumers' scripts run" bind:checked={trustScripts} disabled={running} data-testid="consumers-scripts" />
        <Checkbox label="Allow writes" bind:checked={allowWrites} disabled={running} data-testid="consumers-writes" />
        <p class="hint" data-testid="consumers-writes-hint">{WRITES_HINT}</p>
      </div>
    </form>
  {/if}

  {#if error}
    <div data-testid="consumers-error"><Banner tone="danger" title="Could not verify">{error}</Banner></div>
  {/if}

  {#if running}
    <div class="loading" data-testid="consumers-running"><Spinner label="Running" /> Running {chosen.length} {chosen.length === 1 ? "consumer" : "consumers"}.</div>
  {/if}

  {#if report}
    <div class="results" data-testid="consumers-report">
      <p class="summary" data-testid="consumers-summary">{summary(report)}</p>
      {#each report.consumers as r (r.name)}
        <div class="result" data-testid="consumer-result" data-name={r.name} data-broken={isBroken(r)}>
          <Banner tone={tone(r)} title={headline(r)}>
            <p class="line">{details(r)}</p>
            {#if r.error}<p class="line" data-testid="consumer-error">{r.error}</p>{/if}
            {#each r.failures as f (f.request_path + f.assertion)}
              <div class="failure" data-testid="consumer-failure">
                <p class="line"><Badge kind="method" value={f.request.split(" ")[0] ?? ""} /> <span class="mono">{f.request.split(" ").slice(1).join(" ")}</span></p>
                <p class="line mono">{f.assertion}</p>
                <p class="line">{f.difference}</p>
                <p class="hint">in {f.request_path} of {r.name}</p>
              </div>
            {/each}
          </Banner>
        </div>
      {/each}
    </div>
  {/if}
</section>

<style>
  .consumers {
    container-type: inline-size;
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-5);
    flex: 1;
    min-height: 0;
    overflow: auto;
    box-sizing: border-box;
    padding: var(--rt-space-6);
  }
  h2 {
    margin: 0;
    font: var(--rt-font-title);
  }
  .note,
  .hint {
    margin: 0;
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-3);
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-4);
    min-width: 0;
  }
  .source {
    color: var(--rt-fg-muted);
    overflow-wrap: anywhere;
  }
  .form {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--rt-space-5) var(--rt-space-6);
  }
  .field {
    flex: 1 1 320px;
    min-width: 200px;
  }
  .go {
    display: flex;
    align-items: center;
    min-height: var(--rt-control-md);
  }
  .checks {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-3);
    flex: 1 0 100%;
  }
  .loading {
    display: flex;
    align-items: center;
    gap: var(--rt-space-4);
    color: var(--rt-fg-muted);
  }
  .results {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-4);
  }
  .summary {
    margin: 0;
    font: var(--rt-font-heading);
  }
  .line {
    margin: var(--rt-space-2) 0 0;
    overflow-wrap: anywhere;
  }
  .failure {
    margin-top: var(--rt-space-4);
  }
  .mono {
    font-family: var(--rt-font-mono);
  }
</style>
