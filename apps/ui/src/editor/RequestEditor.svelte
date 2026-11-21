<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Request editor for one request. Owner: wave 3, strand B. Placeholder from the interface commit. -->
<script lang="ts">
  import { getApp } from "../state/app.svelte";

  let { path }: { path: string } = $props();
  const app = getApp();
  const spec = $derived(app.drafts[path]);
</script>

<section data-testid="request-editor" aria-label="Request editor">
  {#if spec}
    <p><strong>{spec.method}</strong> {spec.url}</p>
  {/if}
</section>
