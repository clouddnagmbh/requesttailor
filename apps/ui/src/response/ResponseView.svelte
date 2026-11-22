<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Response of the last send of one request. Owner: wave 3, strand C.
     The companion offer for blocked requests is a separate component (strand G); this view
     shows the error message only. -->
<script lang="ts">
  import SendNotes from "../plugins/SendNotes.svelte";
  import { getApp } from "../state/app.svelte";
  import { describeError } from "./format";
  import ResponsePanel from "./ResponsePanel.svelte";

  let { path }: { path: string } = $props();
  const app = getApp();
  const result = $derived(app.results[path]);
  const response = $derived(result?.report.response ?? null);
</script>

<section data-testid="response-view" aria-label="Response" aria-busy={app.sending[path] ?? false}>
  {#if result && response && result.response_id !== null}
    {#key result.response_id}
      <ResponsePanel {result} {response} backend={app.backend} />
    {/key}
  {:else if result?.report.error}
    <SendNotes notes={result.report.notes ?? []} />
    <p class="error" role="alert" data-testid="response-error">{describeError(result.report.error)}</p>
  {:else if app.sending[path]}
    <p class="note">Sending…</p>
  {:else}
    <p class="note">Send the request to see the response.</p>
  {/if}
</section>

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
  }
  .note {
    margin: 0;
    padding: var(--rt-gap);
    color: var(--rt-fg-muted);
  }
</style>
