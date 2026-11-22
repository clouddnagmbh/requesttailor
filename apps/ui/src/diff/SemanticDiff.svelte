<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- One semantic diff, embeddable: the PR review (strand E) shows it with comments at fields.
     `oncomment`: the user wants to comment on a field (file and 1-based line from
     `FieldChange.line`) (wave 12, strand A). Interface placeholder with the props the other
     strands rely on; the strand replaces the body. -->
<script lang="ts">
  import type { ChangeTarget, CollectionDiff, FieldPath } from "../commands/types";

  let props: {
    diff: CollectionDiff;
    oncomment?: (target: ChangeTarget, field: FieldPath, file: string, line: number) => void;
  } = $props();
</script>

<p class="not-yet" data-testid="wave12-placeholder" data-props={JSON.stringify(props)}>The semantic diff arrives in wave 12.</p>

<style>
  .not-yet {
    padding: calc(var(--rt-gap) * 2);
    color: var(--rt-fg-muted);
  }
</style>
