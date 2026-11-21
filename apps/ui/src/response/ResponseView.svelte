<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Response of the last send of one request. Owner: wave 3, strand C. Placeholder. -->
<script lang="ts">
  import { getApp } from "../state/app.svelte";

  let { path }: { path: string } = $props();
  const app = getApp();
  const result = $derived(app.results[path]);
</script>

<section data-testid="response-view" aria-label="Response">
  {#if result?.report.response}
    <p data-testid="response-status">{result.report.response.status}</p>
  {:else if result?.report.error}
    <p data-testid="response-error">{JSON.stringify(result.report.error)}</p>
  {/if}
</section>
