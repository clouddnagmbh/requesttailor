<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Offers the companion service when the browser blocked a request. Owner: wave 3, strand G.
     Placeholder. -->
<script lang="ts">
  import { getApp } from "../state/app.svelte";

  let { path }: { path: string } = $props();
  const app = getApp();
  const blocked = $derived.by(() => {
    const e = app.results[path]?.report.error;
    return e?.kind === "transport" && e.message.kind === "blocked";
  });
</script>

{#if blocked}
  <aside data-testid="companion-offer">The browser blocked this request.</aside>
{/if}
