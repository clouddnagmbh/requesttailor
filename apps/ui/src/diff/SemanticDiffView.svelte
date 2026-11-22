<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The semantic diff in the main area (working tree, commit, branch compare) with the line diff
     as a toggle (wave 12, strand A). Interface placeholder with the props the other strands rely
     on; the strand replaces the body. -->
<script lang="ts">
  import type { DiffScope } from "../commands/types";

  let props: { scope: DiffScope } = $props();
</script>

<p class="not-yet" data-testid="wave12-placeholder" data-props={JSON.stringify(props)}>The semantic diff arrives in wave 12.</p>

<style>
  .not-yet {
    padding: calc(var(--rt-gap) * 2);
    color: var(--rt-fg-muted);
  }
</style>
