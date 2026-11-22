<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- History of one request: versions, blame per field, test results per commit; view, send and
     restore an old version (wave 12, strand D). Interface placeholder with the props the other
     strands rely on; the strand replaces the body. -->
<script lang="ts">
  let props: { path: string } = $props();
</script>

<p class="not-yet" data-testid="wave12-placeholder" data-props={JSON.stringify(props)}>The request history arrives in wave 12.</p>

<style>
  .not-yet {
    padding: calc(var(--rt-gap) * 2);
    color: var(--rt-fg-muted);
  }
</style>
