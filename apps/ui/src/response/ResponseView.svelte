<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Response of the last send of one request. Owner: wave 3, strand C.
     The companion offer for blocked requests is a separate component (strand G); this view
     shows the error message only. Wave 7 (strand C): a pre-request script that failed shows
     its error and console output; "set as variable" from the JSON tree opens a dialog. -->
<script lang="ts">
  import SendNotes from "../plugins/SendNotes.svelte";
  import SetVariableDialog from "../scripts/SetVariableDialog.svelte";
  import { getApp } from "../state/app.svelte";
  import ConsolePanel from "./ConsolePanel.svelte";
  import { describeError } from "./format";
  import ResponsePanel from "./ResponsePanel.svelte";

  let { path }: { path: string } = $props();
  const app = getApp();
  const result = $derived(app.results[path]);
  const response = $derived(result?.report.response ?? null);
  const logs = $derived(result?.report.logs ?? []);

  let pick = $state<{ path: string; value: unknown } | null>(null);
</script>

<section data-testid="response-view" aria-label="Response" aria-busy={app.sending[path] ?? false}>
  {#if result && response && result.response_id !== null}
    {#key result.response_id}
      <ResponsePanel {result} {response} {path} backend={app.backend} onpick={(p, value) => (pick = { path: p, value })} />
    {/key}
  {:else if result?.report.error}
    <SendNotes notes={result.report.notes ?? []} />
    <p class="error" role="alert" data-testid="response-error" data-kind={result.report.error.kind}>{describeError(result.report.error)}</p>
    {#if logs.length > 0}
      <h3 class="logs-title">Console</h3>
      <ConsolePanel {logs} />
    {/if}
  {:else if result?.report.flow?.kind === "skip"}
    <SendNotes notes={result.report.notes ?? []} />
    <p class="note" data-testid="response-skipped">A pre-request script skipped this request; nothing was sent.</p>
    {#if logs.length > 0}<ConsolePanel {logs} />{/if}
  {:else if app.sending[path]}
    <p class="note">Sending…</p>
  {:else}
    <p class="note">Send the request to see the response.</p>
  {/if}
</section>

<SetVariableDialog {pick} requestPath={path} onclose={() => (pick = null)} />

<style>
  section {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 320px;
    box-sizing: border-box;
    border-top: 1px solid var(--rt-border);
    background: var(--rt-bg);
    color: var(--rt-fg);
  }
  .error {
    margin: 0;
    padding: var(--rt-gap);
    color: var(--rt-danger);
    white-space: pre-wrap;
  }
  .note {
    margin: 0;
    padding: var(--rt-gap);
    color: var(--rt-fg-muted);
  }
  .logs-title {
    margin: 0;
    padding: 0 var(--rt-gap);
    font-size: var(--rt-size-small);
    font-weight: 600;
    color: var(--rt-fg-muted);
    text-transform: uppercase;
  }
</style>
