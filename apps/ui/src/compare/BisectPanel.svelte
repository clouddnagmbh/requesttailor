<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Bisect with progress (wave 12, strand C). Interface placeholder with the props the other
     strands rely on; the strand replaces the body. -->
<script lang="ts">
  let props: { good?: string; bad?: string } = $props();
</script>

<p class="not-yet" data-testid="wave12-placeholder" data-props={JSON.stringify(props)}>Bisect arrives in wave 12.</p>

<style>
  .not-yet {
    padding: calc(var(--rt-gap) * 2);
    color: var(--rt-fg-muted);
  }
</style>
