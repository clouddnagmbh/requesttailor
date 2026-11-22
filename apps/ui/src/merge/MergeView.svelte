<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Field conflicts of the merge in progress, resolved per field (wave 12, strand B). Interface
     placeholder with the props the other strands rely on; the strand replaces the body. -->
<script lang="ts">
  let props: { onclose?: () => void } = $props();
</script>

<p class="not-yet" data-testid="wave12-placeholder" data-props={JSON.stringify(props)}>The semantic merge arrives in wave 12.</p>

<style>
  .not-yet {
    padding: calc(var(--rt-gap) * 2);
    color: var(--rt-fg-muted);
  }
</style>
